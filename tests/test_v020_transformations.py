from __future__ import annotations

import pytest
from trino_sql_validator import analyze_statements, validate


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT missing_fn(1)",
        "select MISSING_FN ( 1 )",
        "SELECT /* boundary */ missing_fn(1)",
        'SELECT missing_fn("where")',
    ],
)
def test_token_boundary_transformations_preserve_validity_and_warning_name(
    sql: str,
) -> None:
    result = validate(sql, jinja="reject")

    assert result.valid is True
    assert result.unknown_functions == ["missing_fn"]
    assert analyze_statements(sql, jinja="reject").validation == result


def test_warning_coordinates_follow_multiline_transformed_source() -> None:
    sql = "SELECT /* Unicode ☃ */\r\n  missing_fn(1)"

    result = validate(sql, jinja="reject")

    assert result.valid is True
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("missing_fn", 2, 3)
    ]


def test_pattern_window_normalization_preserves_all_expression_warnings() -> None:
    sql = (
        "SELECT last_z OVER ("
        "PARTITION BY missing_partition(x) "
        "ORDER BY missing_order(y) "
        "MEASURES missing_measure(z) AS last_z "
        "ROWS BETWEEN missing_start(1) PRECEDING AND missing_end(1) FOLLOWING "
        "PATTERN (A) DEFINE A AS missing_define(z)) FROM t"
    )

    result = validate(sql, jinja="reject")

    expected_names = [
        "missing_partition",
        "missing_order",
        "missing_measure",
        "missing_start",
        "missing_end",
        "missing_define",
    ]
    assert result.valid is True
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        (name, 1, sql.index(name) + 1) for name in expected_names
    ]
    assert analyze_statements(sql, jinja="reject").validation == result


def test_error_coordinates_follow_crlf_and_unicode_comment() -> None:
    sql = "SELECT /* Unicode ☃ */\r\n sum(x) WITHIN GROUP (ORDER BY x) FROM t"

    result = validate(sql, jinja="reject")

    assert result.valid is False
    assert result.error is not None
    assert (result.error.line, result.error.column) == (2, 9)
    assert result.statement_count == 0
    assert result.warnings == ()


@pytest.mark.parametrize("dialect", ["generic", "hive"])
def test_trino_within_group_restriction_does_not_narrow_other_dialects(
    dialect: str,
) -> None:
    sql = "SELECT sum(x) WITHIN GROUP (ORDER BY x) FROM t"

    assert validate(sql, dialect=dialect, jinja="reject").valid is True
