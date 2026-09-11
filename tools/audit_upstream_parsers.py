"""Compare the public validator with pinned Trino and PrestoDB parser tests."""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import re
import subprocess
import sys
import textwrap
import urllib.parse
import urllib.request
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from functools import cached_property
from pathlib import Path
from typing import Any

from trino_sql_validator import __version__, validate

TRINO_REPOSITORY = "trinodb/trino"
PRESTO_REPOSITORY = "prestodb/presto"

TRINO_TEST_FILES = {
    "parser": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParser.java",
    "errors": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParserErrorHandling.java",
    "functions": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParserFunctions.java",
    "routines": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParserRoutines.java",
    "types": "core/trino-parser/src/test/java/io/trino/sql/parser/TestTypeParser.java",
}

PRESTO_TEST_FILES = {
    "parser": "presto-parser/src/test/java/com/facebook/presto/sql/parser/TestSqlParser.java",
    "errors": "presto-parser/src/test/java/com/facebook/presto/sql/parser/TestSqlParserErrorHandling.java",
    "splitter": "presto-parser/src/test/java/com/facebook/presto/sql/parser/TestStatementSplitter.java",
}

METHOD_PATTERN = re.compile(r"\bvoid\s+(test[A-Za-z0-9_]+)\s*\(")
FORMAT_PATTERN = re.compile(r"%(?:s|d)")


@dataclass(frozen=True)
class Example:
    method: str
    sql: str
    source_file: str = ""
    line: int | None = None
    entry_point: str = ""


@dataclass(frozen=True)
class Extraction:
    examples: list[Example]
    skipped: dict[str, int]
    malformed: list[dict[str, Any]]


@dataclass(frozen=True)
class UpstreamSource:
    repository: str
    ref: str
    root: Path | None

    @cached_property
    def revision(self) -> str:
        if self.root is not None:
            return subprocess.run(
                ["git", "-C", str(self.root), "rev-parse", "HEAD"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
        url = (
            f"https://api.github.com/repos/{self.repository}/commits/"
            f"{urllib.parse.quote(self.ref, safe='')}"
        )
        request = urllib.request.Request(url, headers={"User-Agent": "trino-sql-validator-audit"})
        with urllib.request.urlopen(request) as response:
            revision = json.load(response)["sha"]
        if not isinstance(revision, str) or not revision:
            raise ValueError(f"could not resolve {self.repository}@{self.ref}")
        return revision

    def read(self, relative_path: str) -> str:
        if self.root is not None:
            return (self.root / relative_path).read_text(encoding="utf-8")
        url = f"https://raw.githubusercontent.com/{self.repository}/{self.revision}/{relative_path}"
        with urllib.request.urlopen(url) as response:
            return response.read().decode("utf-8")

    def metadata(self, contents: dict[str, str]) -> dict[str, Any]:
        location = str(self.root.resolve()) if self.root is not None else "github"
        metadata: dict[str, Any] = {
            "repository": self.repository,
            "ref": self.ref,
            "revision": self.revision,
            "source": location,
            "content_sha256": {
                name: hashlib.sha256(content.encode("utf-8")).hexdigest()
                for name, content in sorted(contents.items())
            },
        }
        if self.root is not None:
            metadata["dirty"] = bool(
                subprocess.run(
                    ["git", "-C", str(self.root), "status", "--porcelain"],
                    check=True,
                    capture_output=True,
                    text=True,
                ).stdout
            )
        return metadata


def decode_java_string(raw: str) -> str:
    try:
        return json.loads(f'"{raw}"')
    except json.JSONDecodeError:
        replacements = {
            r"\n": "\n",
            r"\r": "\r",
            r"\t": "\t",
            r"\"": '"',
            r"\\": "\\",
        }
        decoded = raw
        for old, new in replacements.items():
            decoded = decoded.replace(old, new)
        return decoded


def parse_java_text_block(source: str, position: int) -> tuple[str, int] | None:
    if not source.startswith('"""', position):
        return None
    end = source.find('"""', position + 3)
    if end == -1:
        return None
    raw = source[position + 3 : end]
    if raw.startswith("\r\n"):
        raw = raw[2:]
    elif raw.startswith("\n"):
        raw = raw[1:]
    return textwrap.dedent(raw), end + 3


def parse_string_sequence(source: str, position: int) -> tuple[str, int] | None:
    if position >= len(source) or source[position] != '"':
        return None
    if source.startswith('"""', position):
        return parse_java_text_block(source, position)
    parts: list[str] = []
    while position < len(source) and source[position] == '"':
        end = position + 1
        while end < len(source):
            if source[end] == "\\":
                end += 2
                continue
            if source[end] == '"':
                break
            end += 1
        if end >= len(source):
            return None
        parts.append(decode_java_string(source[position + 1 : end]))
        position = end + 1
        while position < len(source) and source[position].isspace():
            position += 1
        if position >= len(source) or source[position] != "+":
            break
        candidate = position + 1
        while candidate < len(source) and source[candidate].isspace():
            candidate += 1
        if candidate >= len(source) or source[candidate] != '"':
            break
        position = candidate
    return "".join(parts), position


def method_index(source: str) -> tuple[list[int], list[str]]:
    marks = [(match.start(), match.group(1)) for match in METHOD_PATTERN.finditer(source)]
    return [mark[0] for mark in marks], [mark[1] for mark in marks]


def method_at(position: int, positions: list[int], names: list[str]) -> str:
    index = bisect.bisect_right(positions, position) - 1
    return names[index] if index >= 0 else "outside_test_method"


def is_negative_wrapper(source: str, position: int) -> bool:
    prefix = source[max(0, position - 300) : position]
    return "assertThatThrownBy" in prefix[prefix.rfind(";") + 1 :]


def unique_examples(examples: Iterable[Example]) -> list[Example]:
    seen: set[str] = set()
    result: list[Example] = []
    for example in examples:
        if example.sql in seen:
            continue
        seen.add(example.sql)
        result.append(example)
    return result


def extract_call_examples(
    source: str,
    call_names: Iterable[str],
    *,
    source_file: str = "",
    exclude_negative_wrappers: bool = False,
) -> Extraction:
    positions, names = method_index(source)
    examples: list[Example] = []
    skipped = {
        "non_literal": 0,
        "format_placeholders": 0,
        "negative_wrapper": 0,
        "empty_sql_api_difference": 0,
    }
    malformed: list[dict[str, Any]] = []
    for call_name in call_names:
        pattern = re.compile(rf"\b{re.escape(call_name)}\s*\(\s*")
        for match in pattern.finditer(source):
            if exclude_negative_wrappers and is_negative_wrapper(source, match.start()):
                skipped["negative_wrapper"] += 1
                continue
            parsed = parse_string_sequence(source, match.end())
            if parsed is None:
                if source.startswith('"""', match.end()):
                    malformed.append(
                        {
                            "entry_point": call_name,
                            "source_file": source_file,
                            "line": source.count("\n", 0, match.start()) + 1,
                        }
                    )
                else:
                    skipped["non_literal"] += 1
                continue
            sql, _ = parsed
            line = source.count("\n", 0, match.start()) + 1
            if not sql.strip():
                skipped["empty_sql_api_difference"] += 1
                continue
            if FORMAT_PATTERN.search(sql):
                skipped["format_placeholders"] += 1
                continue
            examples.append(
                Example(
                    method_at(match.start(), positions, names),
                    sql,
                    source_file,
                    line,
                    call_name,
                )
            )
    return Extraction(unique_examples(examples), skipped, malformed)


def between(source: str, start_marker: str, end_marker: str) -> str:
    start = source.index(start_marker)
    end = source.index(end_marker, start + len(start_marker))
    return source[start:end]


def extract_argument_examples(source: str, section: str, source_file: str = "") -> Extraction:
    if section == "expressions":
        body = between(
            source,
            "private static Stream<Arguments> expressions()",
            "private static Stream<Arguments> statements()",
        )
    else:
        body = between(
            source,
            "private static Stream<Arguments> statements()",
            "    @Test",
        )
    examples: list[Example] = []
    skipped = {"non_literal": 0, "empty_sql_api_difference": 0}
    malformed: list[dict[str, Any]] = []
    pattern = re.compile(r"Arguments\.of\(\s*")
    for match in pattern.finditer(body):
        parsed = parse_string_sequence(body, match.end())
        line = source.count("\n", 0, source.index(body) + match.start()) + 1
        if parsed is None:
            skipped["non_literal"] += 1
        elif parsed[0].strip():
            examples.append(Example(section, parsed[0], source_file, line, "Arguments.of"))
        else:
            skipped["empty_sql_api_difference"] += 1
    return Extraction(unique_examples(examples), skipped, malformed)


def warning_data(result: Any) -> list[dict[str, Any]]:
    return [
        {
            "kind": type(warning).__name__,
            "name": warning.name,
            "line": warning.line,
            "column": warning.column,
        }
        for warning in result.warnings
    ]


def probe(
    examples: Iterable[Example],
    expected_valid: bool,
    wrapper: Callable[[str], str] = lambda sql: sql,
) -> dict[str, Any]:
    mismatches: list[dict[str, Any]] = []
    total = 0
    matched = 0
    for example in examples:
        total += 1
        sql = wrapper(example.sql)
        result = validate(sql)
        if result.valid is expected_valid:
            matched += 1
            continue
        mismatches.append(
            {
                "method": example.method,
                "source_file": example.source_file,
                "source_line": example.line,
                "entry_point": example.entry_point,
                "sql": example.sql,
                "validated_sql": sql,
                "actual_valid": result.valid,
                "error": str(result.error) if result.error is not None else None,
                "warnings": warning_data(result),
            }
        )
    return {"total": total, "matched": matched, "mismatches": mismatches}


def file_inventory(files: dict[str, str], contents: dict[str, str]) -> dict[str, Any]:
    return {
        name: {
            "path": path,
            "lines": contents[name].count("\n") + 1,
            "test_methods": len(METHOD_PATTERN.findall(contents[name])),
        }
        for name, path in files.items()
    }


def extraction_data(extraction: Extraction) -> dict[str, Any]:
    return {
        "extracted": len(extraction.examples),
        "skipped": extraction.skipped,
        "malformed": extraction.malformed,
    }


def run_audit(trino: UpstreamSource, presto: UpstreamSource) -> dict[str, Any]:
    trino_contents = {name: trino.read(path) for name, path in TRINO_TEST_FILES.items()}
    presto_contents = {name: presto.read(path) for name, path in PRESTO_TEST_FILES.items()}

    trino_statements = extract_call_examples(
        trino_contents["parser"],
        ("assertStatement", "statement"),
        source_file=TRINO_TEST_FILES["parser"],
        exclude_negative_wrappers=True,
    )
    trino_expressions = extract_call_examples(
        trino_contents["parser"],
        ("expression",),
        source_file=TRINO_TEST_FILES["parser"],
        exclude_negative_wrappers=True,
    )
    trino_types = extract_call_examples(
        trino_contents["types"],
        ("type",),
        source_file=TRINO_TEST_FILES["types"],
        exclude_negative_wrappers=True,
    )
    trino_direct_invalid = extract_call_examples(
        trino_contents["parser"],
        ("assertStatementIsInvalid",),
        source_file=TRINO_TEST_FILES["parser"],
    )
    trino_error_statements = extract_argument_examples(
        trino_contents["errors"], "statements", TRINO_TEST_FILES["errors"]
    )
    trino_error_expressions = extract_argument_examples(
        trino_contents["errors"], "expressions", TRINO_TEST_FILES["errors"]
    )
    trino_function_statements = extract_call_examples(
        trino_contents["functions"],
        ("statement",),
        source_file=TRINO_TEST_FILES["functions"],
        exclude_negative_wrappers=True,
    )
    trino_routine_statements = extract_call_examples(
        trino_contents["routines"],
        ("statement",),
        source_file=TRINO_TEST_FILES["routines"],
        exclude_negative_wrappers=True,
    )
    trino_function_specifications = extract_call_examples(
        trino_contents["routines"],
        ("functionSpecification",),
        source_file=TRINO_TEST_FILES["routines"],
        exclude_negative_wrappers=True,
    )
    presto_statements = extract_call_examples(
        presto_contents["parser"],
        ("assertStatement",),
        source_file=PRESTO_TEST_FILES["parser"],
        exclude_negative_wrappers=True,
    )

    return {
        "validator_version": __version__,
        "limitations": [
            "Direct Java strings and text blocks passed to known parser helpers are extracted.",
            "Variables, generated inputs and AST-only assertions are explicitly skipped.",
            "PrestoDB is a divergence check, not an acceptance target for dialect=trino.",
        ],
        "trino": {
            "source": trino.metadata(trino_contents),
            "inventory": file_inventory(TRINO_TEST_FILES, trino_contents),
            "extraction": {
                "positive_statements": extraction_data(trino_statements),
                "positive_expressions": extraction_data(trino_expressions),
                "positive_types": extraction_data(trino_types),
                "negative_statements_direct": extraction_data(trino_direct_invalid),
                "negative_statements_error_suite": extraction_data(trino_error_statements),
                "negative_expressions_error_suite": extraction_data(trino_error_expressions),
                "function_statements": extraction_data(trino_function_statements),
                "routine_statements": extraction_data(trino_routine_statements),
                "function_specifications": extraction_data(trino_function_specifications),
            },
            "positive_statements": probe(trino_statements.examples, True),
            "positive_expressions": probe(
                trino_expressions.examples, True, lambda expression: f"SELECT {expression}"
            ),
            "positive_types": probe(
                trino_types.examples, True, lambda data_type: f"SELECT CAST(NULL AS {data_type})"
            ),
            "positive_function_statements": probe(trino_function_statements.examples, True),
            "positive_routine_statements": probe(trino_routine_statements.examples, True),
            "positive_function_specifications": probe(
                trino_function_specifications.examples,
                True,
                lambda sql: f"CREATE {sql}",
            ),
            "negative_statements_direct": probe(trino_direct_invalid.examples, False),
            "negative_statements_error_suite": probe(trino_error_statements.examples, False),
            "negative_expressions_error_suite": probe(
                trino_error_expressions.examples,
                False,
                lambda expression: f"SELECT {expression}",
            ),
        },
        "presto_comparison": {
            "source": presto.metadata(presto_contents),
            "inventory": file_inventory(PRESTO_TEST_FILES, presto_contents),
            "extraction": {"positive_statements": extraction_data(presto_statements)},
            "positive_statements": probe(presto_statements.examples, True),
        },
    }


def print_summary(report: dict[str, Any]) -> None:
    print(f"trino-sql-validator {report['validator_version']}")
    for label, data in (
        ("Trino", report["trino"]),
        ("PrestoDB comparison", report["presto_comparison"]),
    ):
        source = data["source"]
        print(f"\n{label}: {source['repository']} @ {source['ref']} ({source['revision'][:12]})")
        for name, result in data.items():
            if not isinstance(result, dict) or "matched" not in result:
                continue
            expectation = "accepted" if name.startswith("positive") else "rejected"
            print(
                f"  {name}: {result['matched']}/{result['total']} {expectation}; "
                f"mismatches={len(result['mismatches'])}"
            )


def mismatch_id(section: str, mismatch: dict[str, Any]) -> str:
    sql = mismatch.get("validated_sql", mismatch.get("sql", ""))
    digest = hashlib.sha256(str(sql).encode("utf-8")).hexdigest()[:16]
    return f"{section}:{digest}"


def build_baseline(report: dict[str, Any]) -> dict[str, Any]:
    trino = report["trino"]
    cases = {}
    for section, result in trino.items():
        if not isinstance(result, dict) or "mismatches" not in result:
            continue
        cases[section] = {
            "minimum_total": result["total"],
            "allowed_mismatches": sorted(
                mismatch_id(section, mismatch) for mismatch in result["mismatches"]
            ),
        }
    return {
        "repository": trino["source"]["repository"],
        "revision": trino["source"]["revision"],
        "cases": cases,
    }


def baseline_regressions(report: dict[str, Any], baseline: dict[str, Any]) -> list[str]:
    regressions = []
    trino = report["trino"]
    if baseline.get("repository") != trino["source"]["repository"]:
        regressions.append("baseline repository does not match the audited Trino repository")
    if baseline.get("revision") != trino["source"]["revision"]:
        regressions.append("baseline revision does not match the resolved Trino revision")
    baseline_cases = baseline.get("cases", {})
    for section, expected in baseline_cases.items():
        result = trino.get(section)
        if not isinstance(result, dict) or "mismatches" not in result:
            regressions.append(f"missing audited section: {section}")
            continue
        if result["total"] < expected["minimum_total"]:
            regressions.append(
                f"{section}: extracted {result['total']} cases, expected at least "
                f"{expected['minimum_total']}"
            )
    for section, result in trino.items():
        if not isinstance(result, dict) or "mismatches" not in result:
            continue
        expected = baseline_cases.get(section, {"allowed_mismatches": []})
        allowed = set(expected["allowed_mismatches"])
        current = {mismatch_id(section, mismatch) for mismatch in result["mismatches"]}
        regressions.extend(f"new mismatch: {item}" for item in sorted(current - allowed))
    extraction = trino.get("extraction", {})
    for section, result in extraction.items():
        for malformed in result.get("malformed", []):
            regressions.append(
                f"malformed extraction: {section}:"
                f"{malformed.get('source_file')}:{malformed.get('line')}"
            )
    return regressions


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trino-ref", default="483")
    parser.add_argument("--presto-ref", default="0.299")
    parser.add_argument("--trino-root", type=Path)
    parser.add_argument("--presto-root", type=Path)
    parser.add_argument("--format", choices=("summary", "json"), default="summary")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--fail-on-regression", action="store_true")
    parser.add_argument("--print-baseline", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = run_audit(
        UpstreamSource(TRINO_REPOSITORY, args.trino_ref, args.trino_root),
        UpstreamSource(PRESTO_REPOSITORY, args.presto_ref, args.presto_root),
    )
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
