from __future__ import annotations

import json
import sys
from importlib import import_module
from pathlib import Path

import pytest
from trino_sql_validator import ValidationResult, analyze_statements, validate

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
builder = import_module("tools.build_v020_transformations")
PATH = ROOT / "tests/cases/trino_483_transformations.json"
PAYLOAD = json.loads(PATH.read_text(encoding="utf-8"))
CASES = PAYLOAD["cases"]


def warning_data(result: ValidationResult) -> list[dict[str, object]]:
    return [
        {
            "kind": type(warning).__name__.removesuffix("Warning").lower(),
            "name": warning.name,
            "line": warning.line,
            "column": warning.column,
        }
        for warning in result.warnings
    ]


def test_transformation_matrix_contract() -> None:
    families = {
        "within_group",
        "table_function",
        "json_encoding",
        "json_table_plan",
        "window_pattern",
        "row_pattern",
        "grammar_edges",
    }
    cells = {
        (case["family"], case["polarity"], case["newline"]) for case in CASES
    }

    assert PAYLOAD == builder.build_payload(
        json.loads((ROOT / "tests/cases/trino_483_composition.json").read_text())
    )
    assert len(CASES) == PAYLOAD["summary"]["total"] == 28
    assert len({case["id"] for case in CASES}) == 28
    assert len({case["sql"] for case in CASES}) == 28
    assert cells == {
        (family, polarity, newline)
        for family in families
        for polarity in ("positive", "negative")
        for newline in ("lf", "crlf")
    }
    assert all("/* Unicode ☃ */" in case["sql"] for case in CASES)
    assert all('"' in case["sql"] for case in CASES if case["polarity"] == "positive")


@pytest.mark.parametrize("case", CASES, ids=[case["id"] for case in CASES])
def test_transformed_trino_case(case: dict[str, object]) -> None:
    sql = case["sql"]
    expected = case["expected"]
    result = validate(sql, dialect="trino", jinja="reject")

    assert result.valid is expected["valid"], result.error
    assert result.statement_count == expected["statement_count"]
    assert warning_data(result) == expected["warnings"]
    assert analyze_statements(sql, dialect="trino", jinja="reject").validation == result
    if result.valid:
        assert result.error is None
        return
    assert result.error is not None
    location = expected["error_location"]
    assert (result.error.line, result.error.column) == (
        location["line"],
        location["column"],
    )
    assert sql.startswith(location["text"], location["offset"])
