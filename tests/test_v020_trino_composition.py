from __future__ import annotations

import json
from pathlib import Path

import pytest
from trino_sql_validator import ValidationResult, analyze_statements, validate

ROOT = Path(__file__).resolve().parent.parent
MATRIX = json.loads(
    (ROOT / "tests/cases/trino_483_composition.json").read_text(encoding="utf-8")
)
CASES = MATRIX["cases"]


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


def test_composition_matrix_contract() -> None:
    cells = {(case["family"], case["polarity"], case["wrapper"]) for case in CASES}

    assert len(CASES) == MATRIX["summary"]["total"] == 84
    assert len({case["id"] for case in CASES}) == 84
    assert len({case["sql"] for case in CASES}) == 84
    assert len(cells) == 7 * 2 * 6
    assert sum(case["expected"]["valid"] for case in CASES) == 42
    assert all(
        case["oracle"]["status"]
        == ("accepted" if case["expected"]["valid"] else "rejected")
        for case in CASES
    )


@pytest.mark.parametrize("case", CASES, ids=[case["id"] for case in CASES])
def test_trino_family_wrapper_composition(case: dict[str, object]) -> None:
    result = validate(case["sql"], dialect="trino", jinja="reject")
    expected = case["expected"]

    assert result.valid is expected["valid"], result.error
    assert result.statement_count == expected["statement_count"]
    assert warning_data(result) == expected["warnings"]
    assert analyze_statements(
        case["sql"], dialect="trino", jinja="reject"
    ).validation == result

    if result.valid:
        assert result.error is None
        return
    assert result.error is not None
    assert result.error.line == 1
    anchor = expected["error_location"]
    assert result.error.column == anchor["offset"] + 1
    assert case["sql"].startswith(anchor["text"], anchor["offset"])
