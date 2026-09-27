"""Run the optional Trino 483 Java parser oracle over a versioned case file."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = 2
ENTRY_POINTS = {
    "createStatement",
    "createExpression",
    "createType",
    "createFunctionSpecification",
    "createRowPattern",
    "createPathSpecification",
}
MAIN_CLASS = "dev.trino_sql_validator.TrinoParserOracle"


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def load_cases(path: Path) -> list[dict[str, Any]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != PROTOCOL_VERSION:
        raise ValueError(f"unsupported oracle protocol: {payload.get('schema_version')!r}")
    cases = payload.get("cases")
    if not isinstance(cases, list):
        raise TypeError("oracle input must contain a cases list")
    normalized: list[dict[str, Any]] = []
    ids: set[str] = set()
    for case in cases:
        if not isinstance(case, dict):
            raise TypeError("each oracle case must be an object")
        case_id = case.get("id")
        entry_point = case.get("entry_point")
        sql = case.get("sql")
        if not isinstance(case_id, str) or not case_id or case_id in ids:
            raise ValueError("oracle case IDs must be nonempty and unique")
        if entry_point not in ENTRY_POINTS:
            raise ValueError(f"unsupported entry point for {case_id}: {entry_point!r}")
        if not isinstance(sql, str):
            raise TypeError(f"SQL for {case_id} must be a string")
        ids.add(case_id)
        raw_sql = case.get("raw_sql", sql)
        if not isinstance(raw_sql, str):
            raise TypeError(f"raw SQL for {case_id} must be a string")
        normalized_case = {
            "id": case_id,
            "entry_point": entry_point,
            "sql": sql,
            "raw_sql": raw_sql,
            "input_sha256": sha256_bytes(sql.encode("utf-8")),
            "raw_input_sha256": sha256_bytes(raw_sql.encode("utf-8")),
            "preparation": case.get("preparation", "none"),
        }
        if "expected_status" in case:
            if case["expected_status"] not in {"accepted", "rejected"}:
                raise ValueError(f"invalid expected status for {case_id}")
            normalized_case["expected_status"] = case["expected_status"]
        if isinstance(case.get("source"), str):
            normalized_case["source"] = case["source"]
        for field in (
            "compatibility_reason",
            "parser_expected",
            "validator_expected",
            "warning_names",
            "engine_evidence",
        ):
            if field in case:
                normalized_case[field] = case[field]
        normalized.append(normalized_case)
    return normalized


def encode_cases(cases: list[dict[str, Any]]) -> str:
    return "".join(
        f"{case['id']}\t{case['entry_point']}\t"
        f"{base64.b64encode(case['sql'].encode('utf-8')).decode('ascii')}\n"
        for case in cases
    )


def parse_output(output: str, cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    expected = {case["id"]: case for case in cases}
    outcomes: dict[str, dict[str, Any]] = {}
    for line in output.splitlines():
        fields = line.split("\t")
        case_id = fields[0]
        if case_id not in expected or case_id in outcomes or len(fields) < 2:
            raise RuntimeError(f"invalid oracle response line: {line!r}")
        status = fields[1].lower()
        case = expected[case_id]
        outcome: dict[str, Any] = {
            "id": case_id,
            "entry_point": case["entry_point"],
            "input_sha256": case.get("input_sha256")
            or sha256_bytes(case["sql"].encode("utf-8")),
            "raw_input_sha256": case.get("raw_input_sha256")
            or sha256_bytes(case.get("raw_sql", case["sql"]).encode("utf-8")),
            "status": status,
        }
        if status == "rejected" and len(fields) == 6:
            outcome.update(
                error_class=fields[2],
                line=int(fields[3]),
                column=int(fields[4]),
                message=base64.b64decode(fields[5]).decode("utf-8"),
            )
        elif status == "infrastructure_error" and len(fields) == 4:
            outcome["error_class"] = fields[2]
            outcome["message"] = base64.b64decode(fields[3]).decode("utf-8")
        elif status != "accepted" or len(fields) != 2:
            raise RuntimeError(f"invalid oracle response line: {line!r}")
        outcomes[case_id] = outcome
    missing = expected.keys() - outcomes.keys()
    if missing:
        raise RuntimeError(f"oracle omitted case IDs: {sorted(missing)}")
    return [outcomes[case["id"]] for case in cases]


def run_oracle(
    cases: list[dict[str, Any]], oracle_dir: Path, java: str, timeout: float
) -> dict[str, Any]:
    classes = oracle_dir / "target" / "classes"
    classpath_file = oracle_dir / "target" / "classpath.txt"
    if not classes.is_dir() or not classpath_file.is_file():
        raise RuntimeError("oracle is not built; follow tools/trino_parser_oracle/README.md")
    dependency_classpath = classpath_file.read_text(encoding="utf-8").strip()
    classpath_entries = [Path(value) for value in dependency_classpath.split(os.pathsep) if value]
    for entry in classpath_entries:
        if not entry.is_file():
            raise RuntimeError(f"oracle classpath entry is missing: {entry}")
    source = oracle_dir / "src/main/java/dev/trino_sql_validator/TrinoParserOracle.java"
    pom = oracle_dir / "pom.xml"
    class_files = sorted(classes.rglob("*.class"))
    if not source.is_file() or not pom.is_file() or not class_files:
        raise RuntimeError("oracle provenance inputs are incomplete")
    java_version = subprocess.run(
        [java, "-version"],
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    if java_version.returncode != 0:
        raise RuntimeError("failed to read Java toolchain version")
    completed = subprocess.run(
        [
            java,
            "-cp",
            f"{classes}{os.pathsep}{dependency_classpath}",
            MAIN_CLASS,
        ],
        input=encode_cases(cases),
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"oracle process exited with {completed.returncode}: {completed.stderr.strip()}"
        )
    outcomes = parse_output(completed.stdout, cases)
    expected_mismatches = [
        {
            "id": case["id"],
            "expected_status": case["expected_status"],
            "actual_status": outcome["status"],
        }
        for case, outcome in zip(cases, outcomes, strict=True)
        if "expected_status" in case and case["expected_status"] != outcome["status"]
    ]
    corpus = json.dumps(cases, sort_keys=True, ensure_ascii=False).encode("utf-8")
    class_manifest = [
        {
            "path": str(path.relative_to(classes)),
            "sha256": sha256_file(path),
        }
        for path in class_files
    ]
    return {
        "schema_version": PROTOCOL_VERSION,
        "trino_version": "483",
        "corpus_sha256": hashlib.sha256(corpus).hexdigest(),
        "case_count": len(cases),
        "oracle_provenance": {
            "pom_sha256": sha256_file(pom),
            "source_sha256": sha256_file(source),
            "classpath_file_sha256": sha256_file(classpath_file),
            "classpath_artifacts": [
                {
                    "name": path.name,
                    "size": path.stat().st_size,
                    "sha256": sha256_file(path),
                }
                for path in classpath_entries
            ],
            "classes": class_manifest,
            "java_version": (java_version.stderr or java_version.stdout).strip(),
        },
        "complete": len(outcomes) == len(cases),
        "expected_mismatches": expected_mismatches,
        "outcomes": outcomes,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument(
        "--oracle-dir", type=Path, default=Path("tools/trino_parser_oracle")
    )
    parser.add_argument("--java", default="java")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        report = run_oracle(
            load_cases(args.input), args.oracle_dir, args.java, args.timeout
        )
    except (OSError, TypeError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"oracle infrastructure error: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output is None:
        sys.stdout.write(rendered)
    else:
        args.output.write_text(rendered, encoding="utf-8")
    if any(item["status"] == "infrastructure_error" for item in report["outcomes"]):
        return 2
    return int(bool(report["expected_mismatches"]))


if __name__ == "__main__":
    raise SystemExit(main())
