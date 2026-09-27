from __future__ import annotations

import pytest
from trino_sql_validator import FunctionWarning, validate

USER_REPRODUCTION = "SELECT\n   t.date::date\nfrom dwh.table_name t"


def test_trino_rejects_postgresql_cast_from_user_reproduction() -> None:
    result = validate(USER_REPRODUCTION)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.message == (
        "sql parser error: Trino '::' is only valid in a static method call"
    )
    assert (result.error.line, result.error.column) == (2, 10)
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT x::bigint",
        "SELECT '2024-01-01'::date",
        "SELECT (x)::bigint",
        "SELECT x::bigint[]",
        "SELECT x::",
        "SELECT x::a::b()",
        "SELECT foo().bar::baz()",
        "SELECT a[1].bar::baz()",
        "SELECT (a).bar::baz()",
    ],
)
def test_trino_rejects_double_colon_outside_static_method_calls(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.line == 1
    assert result.error.column == sql.index("::") + 1
    assert result.warnings == ()


@pytest.mark.parametrize("newline", ["\n", "\r\n"])
def test_double_colon_error_location_preserves_newlines(newline: str) -> None:
    sql = newline.join(["SELECT", "   t.date::date", "FROM dwh.table_name t"])

    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert (result.error.line, result.error.column) == (2, 10)


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT catalog.schema.bigint::parse('42')",
        'SELECT "bigint"::"parse"(\'42\')',
        "SELECT bigint::select(value => 1)",
        "SELECT x::decimal(10, 2)",
        "SELECT bigint /* receiver */ :: /* method */ parse('42')",
    ],
)
def test_trino_static_method_calls_remain_valid(sql: str) -> None:
    assert validate(sql).valid is True


def test_static_method_warning_preserves_source_position() -> None:
    result = validate("SELECT bigint::marh(value => 42)")

    assert result.valid is True
    assert result.warnings == (FunctionWarning("marh", line=1, column=16),)


@pytest.mark.parametrize("dialect", ["generic", "hive"])
def test_double_colon_cast_restriction_is_trino_only(dialect: str) -> None:
    assert validate("SELECT x::bigint", dialect=dialect).valid is True


def test_double_colon_error_in_second_statement_uses_source_coordinates() -> None:
    result = validate("SELECT 1;\nSELECT t.date::date FROM dwh.table_name t")

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert (result.error.line, result.error.column) == (2, 14)
    assert result.warnings == ()
