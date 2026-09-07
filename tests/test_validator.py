"""Integration tests for the trino_sql_validator public API."""

from __future__ import annotations

from pathlib import Path

import pytest
from trino_sql_validator import Error, ValidationResult, validate, validate_file

FIXTURES = Path(__file__).parent / "fixtures"


def test_package_exports_version() -> None:
    from trino_sql_validator import __version__

    assert isinstance(__version__, str)
    assert len(__version__.split(".")) == 3


def test_valid_string_with_multiple_statements() -> None:
    result = validate("SELECT 1; SELECT * FROM t WHERE a > 0; DROP TABLE x;")
    assert isinstance(result, ValidationResult)
    assert result.valid is True
    assert result.statement_count == 3
    assert result.error is None
    assert bool(result) is True


def test_valid_string_single_statement() -> None:
    result = validate("SELECT a, b FROM t GROUP BY a, b")
    assert result.valid is True
    assert result.statement_count == 1


def test_invalid_string_returns_error_value() -> None:
    result = validate("SELECT * FORM t")
    assert result.valid is False
    assert result.statement_count == 0
    assert isinstance(result.error, Error)
    assert isinstance(result.error.message, str)
    assert result.error.line == 1
    assert result.error.column is not None


def test_invalid_does_not_raise() -> None:
    # bad syntax must be returned as a value, never raised
    result = validate("THIS IS NOT SQL AT ALL ((")
    assert result.valid is False


def test_empty_string_is_valid_zero_statements() -> None:
    result = validate("")
    assert result.valid is True
    assert result.statement_count == 0


def test_comments_only_is_zero_statements() -> None:
    result = validate("-- header\n/* block\ntwo lines */")
    assert result.valid is True
    assert result.statement_count == 0


def test_trailing_semicolon_ok() -> None:
    result = validate("SELECT 1;")
    assert result.valid is True
    assert result.statement_count == 1


def test_validate_file_multi_statement() -> None:
    result = validate_file(FIXTURES / "valid_multi.sql")
    assert result.valid is True
    assert result.statement_count == 3


def test_validate_file_invalid_statement() -> None:
    result = validate_file(FIXTURES / "invalid_one.sql")
    assert result.valid is False
    assert result.error is not None
    assert "FORM" in result.error.message


def test_validate_file_accepts_pathlib_path() -> None:
    result = validate_file(FIXTURES / "valid_multi.sql")
    assert result.valid is True


def test_validate_file_trino_specific_syntax() -> None:
    result = validate_file(FIXTURES / "trino_specific.sql")
    assert result.valid is True


def test_validate_file_ddl() -> None:
    result = validate_file(FIXTURES / "ddl_multi.sql")
    assert result.valid is True
    assert result.statement_count == 3


def test_validate_file_empty() -> None:
    result = validate_file(FIXTURES / "empty.sql")
    assert result.valid is True
    assert result.statement_count == 0


def test_validate_file_missing_raises() -> None:
    with pytest.raises(ValueError):
        validate_file(FIXTURES / "does_not_exist.sql")


def test_unknown_dialect_raises() -> None:
    with pytest.raises(ValueError):
        validate("SELECT 1", dialect="mysql")


def test_supported_dialects_have_different_strictness() -> None:
    # backquoted identifiers are accepted by generic/hive but rejected by trino
    result_trino = validate("SELECT `a` FROM t", dialect="trino")
    assert result_trino.valid is False

    result_generic = validate("SELECT `a` FROM t", dialect="generic")
    assert result_generic.valid is True


def test_known_functions_produce_no_warnings() -> None:
    result = validate("SELECT round(1.5), array_agg(x), count(*), sum(y) FROM t")
    assert result.valid is True
    assert result.warnings == ()
    assert result.unknown_functions == []


def test_unknown_function_reports_warning() -> None:
    result = validate("SELECT marh(1.5)")
    assert result.valid is True
    assert len(result.warnings) == 1
    warning = result.warnings[0]
    assert warning.name == "marh"
    assert warning.line == 1
    assert warning.column == 8
    assert result.unknown_functions == ["marh"]


def test_unknown_function_detection_is_case_insensitive() -> None:
    result = validate("SELECT POLLUTION(x)")
    assert result.unknown_functions == ["pollution"]


def test_nested_call_unknown_function() -> None:
    result = validate("SELECT round(marh(x))")
    assert result.unknown_functions == ["marh"]


def test_qualified_unknown_function() -> None:
    result = validate("SELECT schema.foobar(y) FROM t")
    assert result.unknown_functions == ["foobar"]


def test_unknown_function_position_on_later_line() -> None:
    result = validate("SELECT 1\nFROM t\nWHERE x = zort(2)")
    assert result.unknown_functions == ["zort"]
    assert result.warnings[0].line == 3
    assert result.warnings[0].column == 11


def test_function_checking_skipped_for_non_trino_dialect() -> None:
    result = validate("SELECT marh(1)", dialect="generic")
    assert result.valid is True
    assert result.warnings == ()

    result_hive = validate("SELECT marh(1)", dialect="hive")
    assert result_hive.valid is True
    assert result_hive.warnings == ()


def test_known_types_produce_no_warnings() -> None:
    result = validate(
        "CREATE TABLE t (a bigint, b varchar, c decimal(10,2), d boolean, e int, "
        "f double, g timestamp, h varbinary)"
    )
    assert result.valid is True
    assert result.warnings == ()
    assert result.unknown_types == []


def test_unknown_type_in_column_definition_reports_warning() -> None:
    result = validate("CREATE TABLE t (a bignum)")
    assert result.valid is True
    assert len(result.warnings) == 1
    warning = result.warnings[0]
    assert warning.name == "bignum"
    assert warning.line == 1
    assert result.unknown_types == ["bignum"]
    assert result.unknown_functions == []


def test_unknown_type_in_cast_reports_warning() -> None:
    result = validate("SELECT CAST(x AS bignum) FROM t")
    assert result.valid is True
    assert result.unknown_types == ["bignum"]


def test_unknown_type_in_view_and_alter() -> None:
    result = validate(
        "CREATE VIEW v AS SELECT CAST(x AS meep) FROM t; "
        "ALTER TABLE t ADD COLUMN c zop; "
        "ALTER TABLE t ALTER COLUMN c SET DATA TYPE woop"
    )
    assert result.valid is True
    assert result.unknown_types == ["meep", "zop", "woop"]


def test_type_checking_is_case_insensitive() -> None:
    result = validate("CREATE TABLE t (a BigNum)")
    assert result.unknown_types == ["bignum"]


def test_type_warnings_are_distinct_from_function_warnings() -> None:
    from trino_sql_validator import FunctionWarning, TypeWarning

    result = validate("CREATE TABLE t (a bignum, b bigint); SELECT marh(1)")
    assert result.unknown_types == ["bignum"]
    assert result.unknown_functions == ["marh"]
    assert isinstance(result.warnings[0], TypeWarning)
    assert isinstance(result.warnings[1], FunctionWarning)


def test_type_checking_skipped_for_non_trino_dialect() -> None:
    result = validate("CREATE TABLE t (a bignum)", dialect="generic")
    assert result.valid is True
    assert result.warnings == ()


def test_invalid_sql_has_no_warnings() -> None:
    result = validate("SELECT marh(1 FORM")
    assert result.valid is False
    assert result.error is not None
    assert result.warnings == ()


def test_trino_only_functions_are_known() -> None:
    result = validate(
        "SELECT approx_distinct(x), approx_percentile(y, 0.9), "
        "bing_tile(x), json_format(z) FROM t"
    )
    assert result.warnings == ()


@pytest.mark.parametrize(
    ("filename", "statement_count"),
    [
        ("trino_reports_optimize.sql", 17),
        ("trino_iris_queries.sql", 18),
        ("trino_tpch_queries.sql", 39),
    ],
)
def test_complex_trino_fixtures_validate_locally(filename: str, statement_count: int) -> None:
    result = validate_file(FIXTURES / filename)
    assert result.valid is True
    assert result.statement_count == statement_count


@pytest.mark.parametrize(
    ("filename", "line", "column"),
    [
        ("trino_dbt_customers.sql", 20, 10),
        ("trino_reports_tests_schema.sql", 14, 21),
    ],
)
def test_complex_fixtures_preserve_documented_limitations(
    filename: str, line: int, column: int
) -> None:
    result = validate_file(FIXTURES / filename)
    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.line == line
    assert result.error.column == column


def test_create_function_checks_return_type_and_body() -> None:
    result = validate("CREATE FUNCTION f() RETURNS bignum RETURN marh(1)")
    assert result.valid is True
    assert result.unknown_types == ["bignum"]
    assert result.unknown_functions == ["marh"]


def test_trino_statement_rejects_unbalanced_groups() -> None:
    result = validate("ALTER BRANCH b SET RETENTION (3")
    assert result.valid is False


def test_prepare_normalization_preserves_warning_columns() -> None:
    result = validate("PREPARE p FROM SELECT marh(1)")
    assert result.valid is True
    assert result.unknown_functions == ["marh"]
    assert result.warnings[0].column == 23
