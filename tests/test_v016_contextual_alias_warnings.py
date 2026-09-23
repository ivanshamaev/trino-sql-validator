from __future__ import annotations

import pytest
from trino_sql_validator import AliasWarning, FunctionWarning, TypeWarning, validate


@pytest.mark.parametrize("alias", ["all", "OVER", "Partition", "return", "At"])
@pytest.mark.parametrize("template", ["SELECT 1 AS {}", "SELECT 1 {}"])
def test_contextual_projection_alias_is_valid_with_a_warning(
    alias: str, template: str
) -> None:
    sql = template.format(alias)
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == (
        AliasWarning(alias.lower(), line=1, column=sql.index(alias) + 1),
    )
    assert result.ambiguous_aliases == [alias.lower()]


@pytest.mark.parametrize(
    ("sql", "line", "column"),
    [
        ("SELECT * FROM orders AS AT", 1, 25),
        ("SELECT * FROM orders at", 1, 22),
        ("WITH At AS (SELECT 1) SELECT * FROM At", 1, 6),
        ("WITH t(AT) AS (SELECT 1) SELECT * FROM t", 1, 8),
    ],
)
def test_unquoted_at_relation_or_cte_alias_has_a_located_warning(
    sql: str, line: int, column: int
) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == (AliasWarning("at", line=line, column=column),)
    assert result.ambiguous_aliases == ["at"]
    assert result.unknown_functions == []
    assert result.unknown_types == []


@pytest.mark.parametrize("alias", ["all", "OVER", "Partition", "return", "At"])
def test_quoted_contextual_alias_has_no_warning(alias: str) -> None:
    result = validate(f'SELECT 1 AS "{alias}"')

    assert result.valid is True, result.error
    assert result.warnings == ()
    assert result.ambiguous_aliases == []


@pytest.mark.parametrize(
    "sql",
    [
        'SELECT * FROM orders AS "At"',
        'WITH "At" AS (SELECT 1) SELECT * FROM "At"',
        "SELECT At FROM At",
        "SELECT At LOCAL FROM t",
        "SELECT a + At LOCAL FROM t",
    ],
)
def test_at_outside_an_unquoted_alias_has_no_warning(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == ()
    assert result.ambiguous_aliases == []


def test_bare_at_after_an_expression_is_an_implicit_alias() -> None:
    result = validate("SELECT current_timestamp AT")

    assert result.valid is True, result.error
    assert result.warnings == (AliasWarning("at", line=1, column=26),)


def test_at_alias_warnings_keep_source_order_with_catalog_warnings() -> None:
    sql = "SELECT marh(CAST(1 AS bignum))\nAS At"
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == (
        FunctionWarning("marh", line=1, column=8),
        TypeWarning("bignum", line=1, column=23),
        AliasWarning("at", line=2, column=4),
    )


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT current_timestamp AT LOCAL",
        "SELECT current_timestamp AT TIME ZONE 'UTC'",
    ],
)
def test_at_temporal_expressions_do_not_produce_alias_warnings(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT current_timestamp AT UTC",
        "SELECT current_timestamp AT TIME",
    ],
)
def test_malformed_at_temporal_neighbors_remain_invalid(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_non_trino_dialects_do_not_emit_alias_warnings() -> None:
    result = validate("SELECT 1 AS At", dialect="generic")

    assert result.valid is True
    assert result.warnings == ()
