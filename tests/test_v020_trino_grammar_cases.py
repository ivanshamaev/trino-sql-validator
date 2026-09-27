from __future__ import annotations

import json
from pathlib import Path

import pytest
from trino_sql_validator import ValidationResult, validate

ROOT = Path(__file__).resolve().parent.parent
CORPUS_PATH = ROOT / "tests" / "cases" / "trino_483_grammar.json"
CORPUS = json.loads(CORPUS_PATH.read_text(encoding="utf-8"))
CASES = CORPUS["cases"]


def _source_offset(sql: str, line: int, column: int) -> int:
    lines = sql.splitlines(keepends=True)
    if line < 1 or line > len(lines):
        raise AssertionError(f"line {line} is outside the SQL input")
    content = lines[line - 1].removesuffix("\n").removesuffix("\r")
    if column < 1 or column > len(content) + 1:
        raise AssertionError(f"column {column} is outside line {line}")
    return sum(len(item) for item in lines[: line - 1]) + column - 1


def _warnings(result: ValidationResult) -> list[dict[str, object]]:
    return [
        {
            "kind": type(warning).__name__.removesuffix("Warning").lower(),
            "name": warning.name,
            "line": warning.line,
            "column": warning.column,
        }
        for warning in result.warnings
    ]


def test_trino_grammar_corpus_contract() -> None:
    ids = [case["id"] for case in CASES]
    sql = [case["sql"] for case in CASES]

    assert CORPUS["schema_version"] == 2
    assert len(CASES) == CORPUS["summary"]["total"] == 100
    assert len(ids) == len(set(ids))
    assert len(sql) == len(set(sql))
    assert sum(case["expected"]["valid"] for case in CASES) == 69

    for case in CASES:
        expected = case["expected"]
        assert expected["statement_count"] == int(expected["valid"])
        if expected["valid"]:
            assert "error_location" not in expected
            continue
        assert expected["warnings"] == []
        assert expected["error_location_required"] is True
        anchors = expected["error_location"]["anchors"]
        assert anchors
        for anchor in anchors:
            offset = anchor["offset"]
            text = anchor["text"]
            assert 0 <= offset <= len(case["sql"])
            assert case["sql"].startswith(text, offset)
            assert text or offset == len(case["sql"])


@pytest.mark.parametrize("case", CASES, ids=[case["id"] for case in CASES])
def test_trino_grammar_case(case: dict[str, object]) -> None:
    result = validate(case["sql"], dialect="trino", jinja="reject")
    expected = case["expected"]

    assert result.valid is expected["valid"], result.error
    assert result.statement_count == expected["statement_count"]
    assert _warnings(result) == expected["warnings"]

    if expected["valid"]:
        assert result.error is None
        return

    assert result.error is not None
    assert result.error.line is not None
    assert result.error.column is not None
    offset = _source_offset(case["sql"], result.error.line, result.error.column)
    assert offset in {
        anchor["offset"] for anchor in expected["error_location"]["anchors"]
    }
