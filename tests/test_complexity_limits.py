from __future__ import annotations

import subprocess
import sys

import pytest


@pytest.mark.parametrize(
    "source",
    [
        '"SELECT " + " + ".join(["1"] * 5000)',
        (
            '"CREATE FUNCTION f() RETURNS BIGINT " + "BEGIN " * 5000 '
            '+ "RETURN 1;" + " END;" * 5000'
        ),
    ],
    ids=["long-expression-chain", "deep-routine-blocks"],
)
def test_excessive_complexity_returns_invalid_without_crashing(source: str) -> None:
    program = f"""
from trino_sql_validator import validate
sql = {source}
result = validate(sql)
assert not result.valid
assert result.statement_count == 0
assert result.error is not None
assert 'maximum ' in result.error.message
"""
    completed = subprocess.run(
        [sys.executable, "-c", program],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert completed.returncode == 0, completed.stderr
