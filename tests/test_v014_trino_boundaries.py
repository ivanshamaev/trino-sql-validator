from __future__ import annotations

import pytest
from trino_sql_validator import validate

NON_TRINO_SQL = [
    "CREATE INDEX idx ON t(x)",
    "SELECT x FROM t QUALIFY row_number() OVER () = 1",
    "SELECT 1 LIMIT 1 + 2",
    "SELECT 1 LIMIT 1, 2",
    "UPDATE t SET x = 1 FROM s WHERE t.id = s.id",
    "DELETE FROM t USING s WHERE t.id = s.id",
    "INSERT INTO t VALUES (1) RETURNING *",
    "SELECT 1 <=> 1",
    "SELECT 'a' ILIKE 'A'",
    "SELECT * FROM arbitrary_name(1)",
    "ALTER TABLE t ADD COLUMN x BIGINT DEFAULT missing_fn(1)",
    "CREATE TABLE t(x BIGINT DEFAULT missing_fn(1))",
]


@pytest.mark.parametrize("sql", NON_TRINO_SQL)
def test_generic_constructs_are_rejected_only_by_trino(sql: str) -> None:
    assert validate(sql).valid is False
    assert validate(sql, dialect="generic").valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM offset",
        "SELECT * FROM limit",
        "SELECT substring('abc' FROM offset)",
        "SELECT trim('x' FROM offset)",
        "SELECT 1 LIMIT 10",
        "SELECT 1 LIMIT ?",
        "SELECT 1 LIMIT ALL",
        "SELECT 1 OFFSET ?",
        "ALTER TABLE t ADD COLUMN x BIGINT DEFAULT -1",
        "ALTER TABLE t ADD COLUMN x VARCHAR DEFAULT '+33606060606'",
        "CREATE TABLE t(x DATE DEFAULT DATE '2026-01-01')",
    ],
)
def test_trino_neighbors_remain_valid(sql: str) -> None:
    result = validate(sql)

    assert result.valid, result.error


def test_invalid_column_default_reports_original_position() -> None:
    sql = "ALTER TABLE t ADD COLUMN x BIGINT DEFAULT missing_fn(1)"

    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert result.error.line == 1
    assert result.error.column == sql.index("missing_fn") + 1
