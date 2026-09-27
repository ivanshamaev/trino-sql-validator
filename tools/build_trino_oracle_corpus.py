"""Export project SQL into the versioned input protocol for the Java oracle."""

from __future__ import annotations

import argparse
import json
import runpy
from pathlib import Path
from typing import Any

from trino_sql_validator import _prepare_sql

ROOT = Path(__file__).resolve().parents[1]
FIXTURE_INVENTORY = ROOT / "tests/test_fixture_inventory.py"
COMPATIBILITY_CASES = (
    {
        "id": "compatibility:bom-file-prefix",
        "sql": "\ufeffSELECT 1",
        "expected_status": "rejected",
        "validator_expected": True,
        "compatibility_reason": "The public file-input contract accepts one leading UTF-8 BOM.",
    },
    {
        "id": "compatibility:master-materialized-view-execute",
        "sql": "ALTER MATERIALIZED VIEW mv EXECUTE refresh",
        "expected_status": "rejected",
        "validator_expected": True,
        "compatibility_reason": "Forward-compatible Trino master syntax is absent from Trino 483.",
    },
    {
        "id": "compatibility:master-materialized-view-execute-arguments",
        "sql": "ALTER MATERIALIZED VIEW report EXECUTE refresh(value => 1) WHERE true",
        "expected_status": "rejected",
        "validator_expected": True,
        "compatibility_reason": "Forward-compatible Trino master syntax is absent from Trino 483.",
    },
)


def oracle_case(
    *,
    case_id: str,
    sql: str,
    expected_status: str,
    source: str,
    raw_sql: str | None = None,
    preparation: str = "none",
    **metadata: Any,
) -> dict[str, Any]:
    return {
        "id": case_id,
        "entry_point": "createStatement",
        "sql": sql,
        "raw_sql": sql if raw_sql is None else raw_sql,
        "preparation": preparation,
        "expected_status": expected_status,
        "source": source,
        **metadata,
    }


def build_cases() -> list[dict[str, Any]]:
    inventory = runpy.run_path(str(FIXTURE_INVENTORY))
    cases: list[dict[str, Any]] = []
    for filename, index, sql in inventory["POSITIVE_FIXTURE_STATEMENTS"]:
        prepared = _prepare_sql(sql, "auto")
        cases.append(
            oracle_case(
                case_id=f"fixture-positive:{filename}:{index}",
                sql=prepared,
                raw_sql=sql,
                preparation="jinja_auto" if prepared != sql else "none",
                expected_status="accepted",
                source="positive_fixture",
            )
        )
    for filename, index, sql in inventory["INDEPENDENT_INVALID_CASES"]:
        cases.append(
            oracle_case(
                case_id=f"fixture-negative:{filename}:{index:03d}",
                sql=sql,
                expected_status="rejected",
                source="independent_negative_fixture",
            )
        )
    fixtures = inventory["FIXTURES"]
    expectations = inventory["FIXTURE_EXPECTATIONS"]
    splitter = inventory["split_sql_statements"]
    for filename in inventory["INVALID_DATAMART_WARNINGS"]:
        relative = f"invalid_datamarts/{filename}"
        statements = splitter((fixtures / relative).read_text(encoding="utf-8"))
        if len(statements) != 1:
            raise RuntimeError(f"diagnostic fixture must contain one statement: {relative}")
        cases.append(
            oracle_case(
                case_id=f"diagnostic:{filename}",
                sql=statements[0],
                expected_status=(
                    "accepted" if expectations[relative].valid else "rejected"
                ),
                source="diagnostic_datamart",
                parser_expected=expectations[relative].valid,
                validator_expected=expectations[relative].valid,
                warning_names=list(inventory["INVALID_DATAMART_WARNINGS"][filename]),
                engine_evidence="documented_project_fixture_evidence",
            )
        )
    for path, source in [
        (ROOT / "tests/cases/trino_483_grammar.json", "grammar_regression"),
        (ROOT / "tests/cases/trino_483_composition.json", "composition_matrix"),
        (ROOT / "tests/cases/trino_483_transformations.json", "transformation_matrix"),
        (ROOT / "tests/cases/trino_483_mutations.json", "mutation_regression"),
    ]:
        corpus = json.loads(path.read_text(encoding="utf-8"))
        for case in corpus["cases"]:
            cases.append(
                oracle_case(
                    case_id=f"{source}:{case['id']}",
                    sql=case["sql"],
                    expected_status=(
                        "accepted"
                        if case.get("expected_valid", case.get("expected", {}).get("valid"))
                        else "rejected"
                    ),
                    source=source,
                )
            )
    cases.extend(
        oracle_case(
            case_id=case["id"],
            sql=case["sql"],
            expected_status=case["expected_status"],
            source="compatibility_exception",
            validator_expected=case["validator_expected"],
            compatibility_reason=case["compatibility_reason"],
        )
        for case in COMPATIBILITY_CASES
    )
    if len({case["id"] for case in cases}) != len(cases):
        raise RuntimeError("oracle corpus contains duplicate IDs")
    return cases


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    payload = {"schema_version": 2, "cases": build_cases()}
    args.output.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
