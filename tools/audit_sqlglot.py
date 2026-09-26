"""Compare SQLGlot with project fixtures and the pinned Trino parser corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import logging
import platform
import runpy
import sys
from collections import Counter
from collections.abc import Callable, Iterable
from datetime import datetime, timezone
from importlib.metadata import distribution
from pathlib import Path
from typing import Any

from audit_upstream_parsers import (
    TRINO_REPOSITORY,
    TRINO_TEST_FILES,
    Example,
    UpstreamSource,
    extract_argument_examples,
    extract_call_examples,
)
from trino_sql_validator import __version__, _prepare_sql

SQLGLOT_VERSION = "30.19.0"
ROOT = Path(__file__).resolve().parents[1]
FIXTURE_INVENTORY = ROOT / "tests" / "test_fixture_inventory.py"


def ensure_sqlglot_version(installed: str, allow_mismatch: bool = False) -> None:
    if installed != SQLGLOT_VERSION and not allow_mismatch:
        raise RuntimeError(
            f"SQLGlot {installed} is installed; expected exactly {SQLGLOT_VERSION}. "
            "Use --allow-version-mismatch only for an informational drift run."
        )


def _error_details(error: Exception) -> dict[str, Any]:
    details: dict[str, Any] = {"type": type(error).__name__, "description": str(error)}
    errors = getattr(error, "errors", None)
    if isinstance(errors, list) and errors:
        first = errors[0]
        if isinstance(first, dict):
            details["line"] = first.get("line")
            details["column"] = first.get("col")
            details["start_context"] = first.get("start_context")
            details["highlight"] = first.get("highlight")
            details["end_context"] = first.get("end_context")
    return details


def classify_sql(sql: str) -> dict[str, Any]:
    import sqlglot
    from sqlglot.errors import ErrorLevel, ParseError, TokenError

    try:
        trees = sqlglot.parse(sql, read="trino", error_level=ErrorLevel.IMMEDIATE)
    except TokenError as error:
        return {"state": "token_error", "roots": [], "error": _error_details(error)}
    except ParseError as error:
        return {"state": "parse_error", "roots": [], "error": _error_details(error)}
    roots = [type(tree).__name__ for tree in trees if tree is not None]
    has_command = contains_command(trees)
    return {
        "state": "command_fallback" if has_command else "parsed_ast",
        "roots": roots,
        "error": None,
    }


def contains_command(trees: Iterable[Any]) -> bool:
    from sqlglot import exp

    return any(
        isinstance(node, exp.Command)
        for tree in trees
        if tree is not None
        for node in tree.walk()
    )


def stable_case_id(section: str, sql: str) -> str:
    digest = hashlib.sha256(sql.encode("utf-8")).hexdigest()[:16]
    return f"{section}:{digest}"


def _probe(
    section: str,
    cases: Iterable[tuple[str, str, dict[str, Any]]],
    *,
    expected_valid: bool,
) -> dict[str, Any]:
    results = []
    for source_id, sql, provenance in cases:
        outcome = classify_sql(sql)
        results.append(
            {
                "id": stable_case_id(section, sql),
                "source_id": source_id,
                "sql": sql,
                "expected_valid": expected_valid,
                "provenance": provenance,
                **outcome,
            }
        )
    states = Counter(case["state"] for case in results)
    return {
        "total": len(results),
        "expected_valid": expected_valid,
        "state_counts": dict(sorted(states.items())),
        "cases": results,
    }


def _fixture_sections() -> dict[str, dict[str, Any]]:
    inventory = runpy.run_path(str(FIXTURE_INVENTORY))
    positive = inventory["POSITIVE_FIXTURE_STATEMENTS"]
    negative = inventory["INDEPENDENT_INVALID_CASES"]
    prepared_cases = [
        (
            f"{filename}::{index}",
            _prepare_sql(sql, "auto"),
            {"file": filename, "index": index, "prepared": True},
        )
        for filename, index, sql in positive
    ]
    raw_cases = [
        (
            f"{filename}::{index}",
            sql,
            {"file": filename, "index": index, "prepared": False},
        )
        for filename, index, sql in positive
    ]
    negative_cases = [
        (
            f"{filename}::{index:03d}",
            sql,
            {"file": filename, "index": index, "prepared": False},
        )
        for filename, index, sql in negative
    ]
    return {
        "fixture_positive_prepared": _probe(
            "fixture_positive_prepared", prepared_cases, expected_valid=True
        ),
        "fixture_positive_raw": _probe(
            "fixture_positive_raw", raw_cases, expected_valid=True
        ),
        "fixture_negative": _probe("fixture_negative", negative_cases, expected_valid=False),
    }


def _example_cases(
    examples: Iterable[Example],
    wrapper: Callable[[str], str] = lambda sql: sql,
) -> list[tuple[str, str, dict[str, Any]]]:
    return [
        (
            f"{example.source_file}:{example.line}:{example.method}",
            wrapper(example.sql),
            {
                "source_file": example.source_file,
                "source_line": example.line,
                "method": example.method,
                "entry_point": example.entry_point,
                "source_sql": example.sql,
            },
        )
        for example in examples
    ]


def _trino_sections(source: UpstreamSource) -> tuple[dict[str, Any], dict[str, Any]]:
    contents = {name: source.read(path) for name, path in TRINO_TEST_FILES.items()}
    parser = contents["parser"]
    errors = contents["errors"]
    extractions = {
        "trino_positive_statements": extract_call_examples(
            parser,
            ("assertStatement", "statement"),
            source_file=TRINO_TEST_FILES["parser"],
            exclude_negative_wrappers=True,
        ),
        "trino_positive_expressions": extract_call_examples(
            parser,
            ("expression",),
            source_file=TRINO_TEST_FILES["parser"],
            exclude_negative_wrappers=True,
        ),
        "trino_positive_types": extract_call_examples(
            contents["types"],
            ("type",),
            source_file=TRINO_TEST_FILES["types"],
            exclude_negative_wrappers=True,
        ),
        "trino_positive_function_statements": extract_call_examples(
            contents["functions"],
            ("statement",),
            source_file=TRINO_TEST_FILES["functions"],
            exclude_negative_wrappers=True,
        ),
        "trino_positive_routine_statements": extract_call_examples(
            contents["routines"],
            ("statement",),
            source_file=TRINO_TEST_FILES["routines"],
            exclude_negative_wrappers=True,
        ),
        "trino_positive_function_specifications": extract_call_examples(
            contents["routines"],
            ("functionSpecification",),
            source_file=TRINO_TEST_FILES["routines"],
            exclude_negative_wrappers=True,
        ),
        "trino_negative_statements_direct": extract_call_examples(
            parser,
            ("assertStatementIsInvalid",),
            source_file=TRINO_TEST_FILES["parser"],
        ),
        "trino_negative_statements_error_suite": extract_argument_examples(
            errors, "statements", TRINO_TEST_FILES["errors"]
        ),
        "trino_negative_expressions_error_suite": extract_argument_examples(
            errors, "expressions", TRINO_TEST_FILES["errors"]
        ),
    }
    wrappers = {
        "trino_positive_expressions": lambda sql: f"SELECT {sql}",
        "trino_positive_types": lambda sql: f"SELECT CAST(NULL AS {sql})",
        "trino_positive_function_specifications": lambda sql: f"CREATE {sql}",
        "trino_negative_expressions_error_suite": lambda sql: f"SELECT {sql}",
    }
    sections = {}
    for name, extraction in extractions.items():
        expected_valid = "negative" not in name
        sections[name] = _probe(
            name,
            _example_cases(extraction.examples, wrappers.get(name, lambda sql: sql)),
            expected_valid=expected_valid,
        )
        sections[name]["extraction"] = {
            "skipped": extraction.skipped,
            "malformed": extraction.malformed,
        }
    return sections, source.metadata(contents)


def run_audit(source: UpstreamSource, *, allow_version_mismatch: bool = False) -> dict[str, Any]:
    import sqlglot

    ensure_sqlglot_version(sqlglot.__version__, allow_version_mismatch)
    logging.getLogger("sqlglot").setLevel(logging.ERROR)
    trino_sections, trino_metadata = _trino_sections(source)
    return {
        "metadata": {
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "project_version": __version__,
            "python_version": platform.python_version(),
            "sqlglot_version": sqlglot.__version__,
            "sqlglot_expected_version": SQLGLOT_VERSION,
            "sqlglot_source": _sqlglot_source(),
            "parser": "sqlglot.parse",
            "dialect": "trino",
            "error_level": "IMMEDIATE",
            "trino": trino_metadata,
        },
        "sections": {**_fixture_sections(), **trino_sections},
    }


def _sqlglot_source() -> dict[str, Any]:
    direct_url = distribution("sqlglot").read_text("direct_url.json")
    if direct_url is None:
        return {"distribution": "index"}
    try:
        data = json.loads(direct_url)
    except json.JSONDecodeError:
        return {"distribution": "direct", "direct_url": direct_url}
    vcs = data.get("vcs_info", {})
    return {
        "distribution": "direct",
        "url": data.get("url"),
        "vcs_revision": vcs.get("commit_id"),
        "requested_revision": vcs.get("requested_revision"),
    }


def _digest_rows(rows: Iterable[str]) -> str:
    return hashlib.sha256("\n".join(sorted(rows)).encode("utf-8")).hexdigest()


def build_baseline(report: dict[str, Any]) -> dict[str, Any]:
    cases = {}
    for name, section in sorted(report["sections"].items()):
        items = section["cases"]
        cases[name] = {
            "minimum_total": section["total"],
            "corpus_sha256": _digest_rows(case["sql"] for case in items),
            "outcome_sha256": _digest_rows(
                f"{case['id']}:{case['state']}" for case in items
            ),
            "state_counts": section["state_counts"],
        }
    metadata = report["metadata"]
    return {
        "sqlglot_version": metadata["sqlglot_version"],
        "trino_repository": metadata["trino"]["repository"],
        "trino_revision": metadata["trino"]["revision"],
        "trino_content_sha256": metadata["trino"].get("content_sha256", {}),
        "cases": cases,
    }


def baseline_regressions(report: dict[str, Any], baseline: dict[str, Any]) -> list[str]:
    regressions = []
    metadata = report["metadata"]
    if baseline.get("sqlglot_version") != metadata["sqlglot_version"]:
        regressions.append("baseline SQLGlot version does not match the installed version")
    if baseline.get("trino_repository") != metadata["trino"]["repository"]:
        regressions.append("baseline repository does not match the audited Trino repository")
    if baseline.get("trino_revision") != metadata["trino"]["revision"]:
        regressions.append("baseline revision does not match the resolved Trino revision")
    if baseline.get("trino_content_sha256") != metadata["trino"].get("content_sha256", {}):
        regressions.append("baseline Trino source hashes do not match audited files")
    current = build_baseline(report)["cases"]
    expected_cases = baseline.get("cases", {})
    for name, expected in expected_cases.items():
        actual = current.get(name)
        if actual is None:
            regressions.append(f"missing audited section: {name}")
            continue
        if actual["minimum_total"] < expected["minimum_total"]:
            regressions.append(
                f"{name}: audited {actual['minimum_total']} cases, expected at least "
                f"{expected['minimum_total']}"
            )
        if actual["corpus_sha256"] != expected["corpus_sha256"]:
            regressions.append(f"{name}: corpus hash changed")
        if actual["outcome_sha256"] != expected["outcome_sha256"]:
            regressions.append(f"{name}: SQLGlot outcome changed")
        if actual["state_counts"] != expected["state_counts"]:
            regressions.append(f"{name}: state counts changed")
    for name in sorted(set(current) - set(expected_cases)):
        regressions.append(f"new audited section is absent from baseline: {name}")
    for name, section in report["sections"].items():
        for malformed in section.get("extraction", {}).get("malformed", []):
            regressions.append(f"malformed extraction in {name}: {malformed}")
    return regressions


def print_summary(report: dict[str, Any]) -> None:
    metadata = report["metadata"]
    trino = metadata["trino"]
    print(
        f"trino-sql-validator {metadata['project_version']}; "
        f"SQLGlot {metadata['sqlglot_version']}"
    )
    print(f"Trino: {trino['repository']} @ {trino['ref']} ({trino['revision'][:12]})")
    sqlglot_revision = metadata["sqlglot_source"].get("vcs_revision")
    if sqlglot_revision:
        print(f"SQLGlot source revision: {sqlglot_revision}")
    for name, section in report["sections"].items():
        counts = ", ".join(
            f"{state}={count}" for state, count in section["state_counts"].items()
        )
        print(f"  {name}: total={section['total']}; {counts}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trino-ref", default="483")
    parser.add_argument("--trino-root", type=Path)
    parser.add_argument("--format", choices=("summary", "json"), default="summary")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--fail-on-regression", action="store_true")
    parser.add_argument("--print-baseline", action="store_true")
    parser.add_argument("--allow-version-mismatch", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        report = run_audit(
            UpstreamSource(TRINO_REPOSITORY, args.trino_ref, args.trino_root),
            allow_version_mismatch=args.allow_version_mismatch,
        )
    except (ImportError, RuntimeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if args.print_baseline:
        json.dump(build_baseline(report), sys.stdout, ensure_ascii=False, indent=2)
        print()
        return 0
    if args.format == "json":
        json.dump(report, sys.stdout, ensure_ascii=False, indent=2)
        print()
    else:
        print_summary(report)
    if args.fail_on_regression:
        if args.baseline is None:
            print("error: --fail-on-regression requires --baseline", file=sys.stderr)
            return 2
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
        regressions = baseline_regressions(report, baseline)
        if regressions:
            for regression in regressions:
                print(f"regression: {regression}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
