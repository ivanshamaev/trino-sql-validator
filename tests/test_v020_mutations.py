from __future__ import annotations

import json
from pathlib import Path

import pytest
from trino_sql_validator import analyze_statements, validate

ROOT = Path(__file__).resolve().parent.parent
PAYLOAD = json.loads(
    (ROOT / "tests/cases/trino_483_mutations.json").read_text(encoding="utf-8")
)
CASES = PAYLOAD["cases"]


def test_mutation_corpus_contract() -> None:
    assert len(CASES) == PAYLOAD["summary"]["total"] == 6
    assert len({case["id"] for case in CASES}) == 6
    assert sum(case["expected_valid"] for case in CASES) == 3
    assert all(case["base_sql"] != case["sql"] for case in CASES)
    assert all(case["removed_token"] for case in CASES)
    assert all(
        case["oracle"]["status"]
        == ("accepted" if case["expected_valid"] else "rejected")
        for case in CASES
    )


@pytest.mark.parametrize("case", CASES, ids=[case["id"] for case in CASES])
def test_native_classified_token_deletion(case: dict[str, object]) -> None:
    result = validate(case["sql"], dialect="trino", jinja="reject")

    assert result.valid is case["expected_valid"]
    assert result.statement_count == int(case["expected_valid"])
    assert analyze_statements(
        case["sql"], dialect="trino", jinja="reject"
    ).validation == result
