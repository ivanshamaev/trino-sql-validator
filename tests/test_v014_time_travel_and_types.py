from __future__ import annotations

import pytest
from trino_sql_validator import FunctionWarning, TypeWarning, validate


@pytest.mark.parametrize(
    "expression",
    ["CAST(1 AS BIGINT)", "(1)", "?", "1 + 2", "'audit'"],
)
def test_iceberg_version_time_travel_accepts_value_expressions(expression: str) -> None:
    result = validate(f"SELECT * FROM t FOR VERSION AS OF {expression}")

    assert result.valid, result.error


def test_time_travel_expression_preserves_function_warning_position() -> None:
    sql = "SELECT * FROM t FOR VERSION AS OF missing_fn(1)"

    result = validate(sql)

    assert result.valid, result.error
    assert result.warnings == (
        FunctionWarning("missing_fn", line=1, column=sql.index("missing_fn") + 1),
    )


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM t FOR VERSION AS OF",
        "SELECT * FROM t FOR TIMESTAMP AS OF",
        "SELECT * FROM t FOR VERSION OF 1",
        "SELECT * FROM t FOR VERSION AS 1",
    ],
)
def test_time_travel_rejects_malformed_neighbors(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CAST(NULL AS bignum ARRAY)",
        "CREATE TABLE t(c bignum ARRAY)",
        "ALTER TABLE t ADD COLUMN c bignum ARRAY",
        "ALTER TABLE t ALTER COLUMN c SET DATA TYPE bignum ARRAY",
        ("CREATE FUNCTION f(x bignum ARRAY) RETURNS bignum ARRAY RETURN x"),
        "DROP FUNCTION f(bignum ARRAY)",
        "PREPARE p FROM CREATE TABLE t(c bignum ARRAY)",
        "SELECT CAST(NULL AS ROW(x bignum ARRAY))",
        "CREATE TABLE t(c ROW(x bignum ARRAY))",
    ],
)
def test_unknown_postfix_array_types_work_in_supported_contexts(sql: str) -> None:
    result = validate(sql)

    assert result.valid, result.error
    warnings = [warning for warning in result.warnings if isinstance(warning, TypeWarning)]
    assert warnings
    assert all(warning.name == "bignum" for warning in warnings)
    assert [warning.column for warning in warnings] == [
        index + 1 for index in range(len(sql)) if sql.startswith("bignum", index)
    ]


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE TABLE t(c ARRAY)",
        "ALTER TABLE t ADD COLUMN c ARRAY",
        "CREATE FUNCTION f(x ARRAY) RETURNS BIGINT RETURN 1",
    ],
)
def test_postfix_array_normalization_does_not_hide_missing_element_type(sql: str) -> None:
    assert validate(sql).valid is False
