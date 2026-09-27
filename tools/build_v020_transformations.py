"""Build deterministic multiline/CRLF transformations for v0.20 syntax families."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
COMPOSITION = ROOT / "tests/cases/trino_483_composition.json"
OUTPUT = ROOT / "tests/cases/trino_483_transformations.json"
FAMILIES = (
    "within_group",
    "table_function",
    "json_encoding",
    "json_table_plan",
    "window_pattern",
    "row_pattern",
    "grammar_edges",
)
POSITIVE_SENTINELS = {
    "within_group": (
        (
            "SELECT listagg(missing_value(x), ',') WITHIN GROUP "
            '(ORDER BY missing_order(x)) AS "where" FROM t'
        ),
        ("missing_value", "missing_order"),
    ),
    "table_function": (
        (
            "SELECT * FROM TABLE(missing_ptf(d => DESCRIPTOR(x missing_type), "
            'v => missing_scalar(1))) AS "where"'
        ),
        ("missing_ptf", "missing_type", "missing_scalar"),
    ),
    "json_encoding": (
        (
            "SELECT JSON_VALUE(missing_json(1) FORMAT JSON ENCODING UTF8, '$x' "
            "PASSING missing_pass(2) FORMAT JSON ENCODING UTF16 AS x RETURNING "
            'missing_type DEFAULT missing_default(0) ON ERROR) AS "where"'
        ),
        ("missing_json", "missing_pass", "missing_type", "missing_default"),
    ),
    "json_table_plan": (
        (
            "SELECT * FROM JSON_TABLE(missing_json(1), '$' AS root COLUMNS "
            "(x missing_type DEFAULT missing_default(0) ON EMPTY, NESTED PATH "
            "'$.a' AS a COLUMNS (y INTEGER)) PLAN (root OUTER a)) AS \"where\""
        ),
        ("missing_json", "missing_type", "missing_default"),
    ),
    "window_pattern": (
        (
            'SELECT "where" OVER (PARTITION BY missing_partition(x) ORDER BY '
            "missing_order(y) MEASURES missing_measure(z) AS \"where\" ROWS "
            "BETWEEN missing_start(1) PRECEDING AND missing_end(1) FOLLOWING "
            "PATTERN (A) DEFINE A AS missing_define(z)) FROM t"
        ),
        (
            "missing_partition",
            "missing_order",
            "missing_measure",
            "missing_start",
            "missing_end",
            "missing_define",
        ),
    ),
    "row_pattern": (
        (
            "SELECT * FROM t MATCH_RECOGNIZE (MEASURES missing_measure(A.x) AS "
            '"where" AFTER MATCH SKIP TO NEXT ROW PATTERN (A $) DEFINE A AS '
            'missing_define(A.x)) AS "from"'
        ),
        ("missing_measure", "missing_define"),
    ),
    "grammar_edges": (
        (
            "SELECT CASE missing_input(x) WHEN BETWEEN missing_lower(1) AND "
            'missing_upper(2) THEN missing_result(3) END AS "where" FROM t'
        ),
        ("missing_input", "missing_lower", "missing_upper", "missing_result"),
    ),
}


def _prefix(newline: str) -> str:
    return f"/* Unicode ☃ */{newline}  "


def build_cases(composition: dict[str, Any]) -> list[dict[str, Any]]:
    negative_roots = {
        case["family"]: case
        for case in composition["cases"]
        if case["polarity"] == "negative" and case["wrapper"] == "root"
    }
    cases = []
    for family in FAMILIES:
        positive_sql, warning_names = POSITIVE_SENTINELS[family]
        negative = negative_roots[family]
        for newline_name, newline in (("lf", "\n"), ("crlf", "\r\n")):
            prefix = _prefix(newline)
            warnings = [
                {
                    "kind": "type" if name == "missing_type" else "function",
                    "name": name,
                    "line": 2,
                    "column": positive_sql.index(name) + 3,
                }
                for name in warning_names
            ]
            cases.append(
                {
                    "id": f"TR-{family}-positive-{newline_name}",
                    "family": family,
                    "polarity": "positive",
                    "newline": newline_name,
                    "sql": prefix + positive_sql,
                    "expected": {
                        "valid": True,
                        "statement_count": 1,
                        "warnings": warnings,
                    },
                    "oracle": {"status": "accepted"},
                }
            )
            anchor = negative["expected"]["error_location"]
            cases.append(
                {
                    "id": f"TR-{family}-negative-{newline_name}",
                    "family": family,
                    "polarity": "negative",
                    "newline": newline_name,
                    "sql": prefix + negative["sql"],
                    "expected": {
                        "valid": False,
                        "statement_count": 0,
                        "warnings": [],
                        "error_location": {
                            "line": 2,
                            "column": anchor["offset"] + 3,
                            "offset": len(prefix) + anchor["offset"],
                            "text": anchor["text"],
                        },
                    },
                    "oracle": {"status": "rejected"},
                }
            )
    return cases


def build_payload(composition: dict[str, Any]) -> dict[str, Any]:
    cases = build_cases(composition)
    source_bytes = json.dumps(composition, sort_keys=True, ensure_ascii=False).encode()
    return {
        "schema_version": 1,
        "source": {
            "trino_ref": "483",
            "composition_sha256": hashlib.sha256(source_bytes).hexdigest(),
            "construction": "Unicode block comment, LF/CRLF, two-space indentation",
        },
        "summary": {"total": len(cases), "positive": 14, "negative": 14},
        "cases": cases,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--composition", type=Path, default=COMPOSITION)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    composition = json.loads(args.composition.read_text(encoding="utf-8"))
    rendered = json.dumps(build_payload(composition), ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if not args.output.is_file() or args.output.read_text(encoding="utf-8") != rendered:
            raise SystemExit(f"transformation corpus is stale: {args.output}")
        return 0
    args.output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
