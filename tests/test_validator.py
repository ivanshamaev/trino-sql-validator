"""Integration tests for the trino_sql_validator public API."""

from __future__ import annotations

from pathlib import Path
from time import perf_counter

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


def test_parser_rejects_excessive_nesting_without_unwinding() -> None:
    sql = "SELECT " + "(" * 300 + "1" + ")" * 300

    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert "maximum nesting depth" in result.error.message


def test_utf8_bom_and_crlf_are_accepted_with_logical_warning_locations() -> None:
    result = validate("\ufeffSELECT 1\r\nUNION ALL\r\nSELECT marh(2)")

    assert result.valid is True, result.error
    assert result.unknown_functions == ["marh"]
    assert (result.warnings[0].line, result.warnings[0].column) == (3, 8)


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CASE WHEN " + " * ".join(str(value) for value in range(1, 91)),
        "SELECT id FROM t WHERE\n" + "(f()\nOR " * 22 + "GROUP BY id",
    ],
    ids=["case-chain", "nested-or-chain"],
)
def test_upstream_backtracking_regressions_finish_quickly(sql: str) -> None:
    started = perf_counter()

    result = validate(sql)

    assert result.valid is False
    assert perf_counter() - started < 2.0


def test_invalid_errors_are_deterministic() -> None:
    sql = "CREATE VIEW report SECURITY OWNER AS SELECT 1 trailing"

    first = validate(sql)
    second = validate(sql)

    assert first == second
    assert first.valid is False


def test_comments_escaped_quotes_and_jinja_preserve_transform_positions() -> None:
    sql = (
        "SELECT\r\n"
        "  (marh('{{ literal }}''s value')).trim() /* adjacent */ "
        "BETWEEN SYMMETRIC {{ lower_bound }} AND 10"
    )

    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_functions == ["marh"]
    assert (result.warnings[0].line, result.warnings[0].column) == (2, 4)


@pytest.mark.parametrize(
    "sql",
    [
        "PREPARE p FROM\nSELECT marh(1)",
        "SELECT CAST(1 AS ARRAY (BIGINT)),\nmarh(1)",
        "SELECT IPADDRESS '10.0.0.1',\nmarh(1)",
        "SELECT * FROM customer FOR VERSION AS OF 'audit'\nWHERE marh(id)",
        "SELECT * FROM (VALUES 'one', 'two') AS t(v)\nWHERE marh(v)",
        "SELECT 1 AS x UNION CORRESPONDING BY (x)\nSELECT marh(2) AS x",
        "SELECT * FROM t PIVOT (sum(v) FOR k IN (1)\nGROUP BY marh(g))",
        "SELECT * FROM a CROSS JOIN NEAREST (FROM b\nMATCH marh(b.ts) <= a.ts)",
        "SELECT ROW(1,\nmarh(2)).* AS (x, y)",
        "SELECT a FROM t\nGROUP BY ALL marh(a)",
        "SELECT\nmarh(ts) AT LOCAL",
        "UPDATE t @ dev\nSET x = marh(1)",
        "SELECT\nmarh(1) BETWEEN SYMMETRIC 0 AND 2",
        "SELECT\n('a').marh()",
        "SELECT\ntrim(BOTH FROM marh(' x '))",
        "SELECT U&'abc#0041' UESCAPE '#',\nmarh(1)",
        "SELECT transform(ARRAY[1], () -> 42),\nmarh(1)",
        "SELECT bigint::parse(value => '42'),\nmarh(1)",
    ],
)
def test_multiline_token_transform_location_matrix(sql: str) -> None:
    result = validate(sql)
    prefix = sql[: sql.index("marh")]

    assert result.valid is True, result.error
    warning = next(warning for warning in result.warnings if warning.name == "marh")
    assert (warning.line, warning.column) == (
        prefix.count("\n") + 1,
        len(prefix.rsplit("\n", maxsplit=1)[-1]) + 1,
    )


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


@pytest.mark.parametrize(
    ("filename", "statement_count"),
    [
        ("datamart_example.sql", 1),
        ("samples.sql", 6),
        ("example-queries.sql", 76),
        ("iceberg_trino_sqldemo.sql", 100),
    ],
)
def test_documented_fixture_corpus_validates(filename: str, statement_count: int) -> None:
    result = validate_file(FIXTURES / filename)
    assert result.valid is True
    assert result.statement_count == statement_count
    assert result.error is None
    if filename != "iceberg_trino_sqldemo.sql":
        assert result.warnings == ()


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


@pytest.mark.parametrize(
    "data_type",
    [
        "TIMESTAMP(p)",
        "TIMESTAMP(p) WITHOUT TIME ZONE",
        "TIMESTAMP(p) WITH TIME ZONE",
        "TIME(p)",
        "TIME(p) WITHOUT TIME ZONE",
        "TIME(p) WITH TIME ZONE",
        "INTERVAL YEAR(1) TO MONTH",
        "INTERVAL DAY(1) TO SECOND(2)",
        "INTERVAL HOUR(1) TO MINUTE",
        "INTERVAL MINUTE(1) TO SECOND(2)",
        "INTERVAL SECOND(1, 2)",
        "MAP<BIGINT, VARCHAR>",
        "MAP<BIGINT, VARCHAR> ARRAY",
        "VARCHAR(7) ARRAY ARRAY",
        "ROW(x BIGINT, z ROW(m ARRAY<BIGINT>, n MAP<DOUBLE, VARCHAR>))",
    ],
)
def test_trino_structural_type_extensions_validate(data_type: str) -> None:
    result = validate(f"SELECT CAST(NULL AS {data_type})")

    assert result.valid is True, result.error
    assert result.warnings == ()


def test_trino_structural_type_extensions_preserve_nested_warning_positions() -> None:
    sql = "SELECT CAST(NULL AS MAP<BIGINT, bignum> ARRAY ARRAY)"
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum"]
    assert [(warning.line, warning.column) for warning in result.warnings] == [
        (1, sql.index("bignum") + 1)
    ]


@pytest.mark.parametrize(
    "data_type",
    [
        "TIMESTAMP()",
        "TIMESTAMP(p, q)",
        "INTERVAL YEAR() TO MONTH",
        "INTERVAL YEAR(1, 2) TO MONTH",
        "INTERVAL DAY(x) TO SECOND",
        "INTERVAL SECOND(1, 2, 3)",
        "INTERVAL DAY(1) TO SECOND()",
        "MAP<BIGINT>",
        "MAP<BIGINT, VARCHAR",
        "BIGINT ARRAY[x]",
    ],
)
def test_trino_structural_type_malformed_neighbors_are_rejected(data_type: str) -> None:
    result = validate(f"SELECT CAST(NULL AS {data_type})")

    assert result.valid is False
    assert result.statement_count == 0
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


def test_dbt_jinja_fixture_validates_in_auto_mode() -> None:
    result = validate_file(FIXTURES / "trino_dbt_customers.sql")
    assert result.valid is True
    assert result.statement_count == 1


def test_jinja_reject_mode_preserves_strict_sql_behavior() -> None:
    result = validate_file(FIXTURES / "trino_dbt_customers.sql", jinja="reject")
    assert result.valid is False


def test_clean_sql_is_unchanged_by_jinja_mode() -> None:
    sql = "SELECT round(value) FROM source"
    assert validate(sql).warnings == validate(sql, jinja="mask").warnings


def test_unknown_jinja_mode_raises() -> None:
    with pytest.raises(ValueError):
        validate("SELECT 1", jinja="unsupported")  # type: ignore[arg-type]


def test_trino_accepts_literal_backslash() -> None:
    result = validate(r"SELECT CAST('\' AS VARCHAR)")
    assert result.valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "WITH RECURSIVE tree AS (SELECT 1 AS id) SELECT * FROM tree",
        "SELECT * FROM (WITH RECURSIVE tree AS (SELECT 1 AS id) SELECT * FROM tree) t",
        "SELECT * FROM (WITH RECURSIVE tree(id) AS (SELECT 1) SELECT * FROM tree) t",
    ],
)
def test_recursive_ctes_validate_in_nested_queries(sql: str) -> None:
    result = validate(sql)
    assert result.valid is True
    assert result.statement_count == 1


def test_create_function_checks_return_type_and_body() -> None:
    result = validate("CREATE FUNCTION f() RETURNS bignum RETURN marh(1)")
    assert result.valid is True
    assert result.unknown_types == ["bignum"]
    assert result.unknown_functions == ["marh"]


def test_create_function_structurally_parses_characteristics_and_parameters() -> None:
    sql = (
        "CREATE FUNCTION f(value bignum)\n"
        "RETURNS woop\n"
        "LANGUAGE SQL\n"
        "NOT DETERMINISTIC\n"
        "RETURNS NULL ON NULL INPUT\n"
        "SECURITY DEFINER\n"
        "COMMENT 'parser coverage'\n"
        "WITH (optimization = true)\n"
        "RETURN marh(value)"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum", "woop"]
    assert result.unknown_functions == ["marh"]
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("bignum", 1, 25),
        ("woop", 2, 9),
        ("marh", 9, 8),
    ]


def test_create_function_treats_dollar_body_as_opaque_language_text() -> None:
    sql = (
        "CREATE FUNCTION external_f(value bignum) RETURNS woop\n"
        "LANGUAGE python\n"
        "AS $$return marh(value) and the word RETURN are opaque$$"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum", "woop"]
    assert result.unknown_functions == []


def test_create_function_parses_compound_control_body_and_warnings() -> None:
    sql = (
        "CREATE FUNCTION routine_f(n bignum)\n"
        "RETURNS woop\n"
        "BEGIN\n"
        "  DECLARE a innerbad DEFAULT marh(1);\n"
        "  IF marh(n) > 2 THEN\n"
        "    SET a = marh(n);\n"
        "  ELSEIF n = 2 THEN\n"
        "    SET a = 2;\n"
        "  ELSE\n"
        "    SET a = 1;\n"
        "  END IF;\n"
        "  WHILE n > 0 DO\n"
        "    SET n = n - 1;\n"
        "  END WHILE;\n"
        "  RETURN marh(a);\n"
        "END"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum", "woop", "innerbad"]
    assert result.unknown_functions == ["marh", "marh", "marh", "marh"]
    assert [(warning.name, warning.line) for warning in result.warnings] == [
        ("bignum", 1),
        ("woop", 2),
        ("innerbad", 4),
        ("marh", 4),
        ("marh", 5),
        ("marh", 6),
        ("marh", 15),
    ]


def test_create_function_supports_all_compound_control_shapes() -> None:
    sql = (
        "CREATE FUNCTION control_f(n bigint) RETURNS bigint BEGIN\n"
        "  DECLARE result bigint DEFAULT 0;\n"
        "  CASE n WHEN 0 THEN SET result = 1; ELSE SET result = 2; END CASE;\n"
        "  CASE WHEN n > 0 THEN SET result = 3; END CASE;\n"
        "  outer_loop: LOOP ITERATE outer_loop; END LOOP;\n"
        "  counting: WHILE n > 0 DO SET n = n - 1; END WHILE;\n"
        "  retry: REPEAT SET n = n + 1; UNTIL n > 0 END REPEAT;\n"
        "  LEAVE outer_loop;\n"
        "  RETURN result;\n"
        "END"
    )

    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE FUNCTION f() RETURNS bigint LANGUAGE SQL LANGUAGE SQL RETURN 1",
        "CREATE FUNCTION f() RETURNS bigint DETERMINISTIC NOT DETERMINISTIC RETURN 1",
        "CREATE FUNCTION f() RETURNS bigint SECURITY OWNER RETURN 1",
        "CREATE FUNCTION f() RETURNS bigint RETURNS NULL INPUT RETURN 1",
        "CREATE FUNCTION f() RETURNS bigint AS 'return 1'",
        "CREATE FUNCTION f() RETURNS bigint AS $tag$return 1$tag$",
        "CREATE FUNCTION f() RETURNS bigint BEGIN RETURN 1 END",
        "CREATE FUNCTION f() RETURNS bigint BEGIN IF true RETURN 1; END IF; END",
        "CREATE FUNCTION f() RETURNS bigint BEGIN DECLARE x bigint RETURN x; END",
        "CREATE FUNCTION f() RETURNS bigint BEGIN LOOP END LOOP; END",
    ],
)
def test_create_function_rejects_malformed_routine_shapes(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_inline_with_function_checks_body_and_hides_local_function_names() -> None:
    sql = (
        "WITH\n"
        "  FUNCTION hello(name VARCHAR)\n"
        "  RETURNS bignum\n"
        "  RETURN marh(name),\n"
        "  FUNCTION bye()\n"
        "  RETURNS BIGINT\n"
        "  RETURN hello('x')\n"
        "SELECT hello('Finn'), bye()"
    )

    result = validate(sql)

    assert result.valid is True
    assert result.statement_count == 1
    assert result.unknown_types == ["bignum"]
    assert result.unknown_functions == ["marh"]
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("bignum", 3, 11),
        ("marh", 4, 10),
    ]


def test_inline_with_function_allows_a_following_cte_query() -> None:
    result = validate(
        "WITH FUNCTION answer() RETURNS BIGINT RETURN 42\n"
        "WITH t AS (SELECT answer() AS value) SELECT value FROM t"
    )

    assert result.valid is True
    assert result.statement_count == 1
    assert result.warnings == ()


def test_inline_with_function_supports_opaque_dollar_body() -> None:
    sql = (
        "WITH FUNCTION external_f(value bignum) RETURNS woop "
        "LANGUAGE python AS $$return marh(value)$$\n"
        "SELECT external_f(1)"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum", "woop"]
    assert result.unknown_functions == []


def test_inline_with_function_supports_compound_sql_body() -> None:
    sql = (
        "WITH FUNCTION local_f(value bignum) RETURNS woop\n"
        "BEGIN\n"
        "  DECLARE result innerbad DEFAULT marh(value);\n"
        "  RETURN marh(result);\n"
        "END\n"
        "SELECT local_f(1)"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum", "woop", "innerbad"]
    assert result.unknown_functions == ["marh", "marh"]
    assert [(warning.name, warning.line) for warning in result.warnings] == [
        ("bignum", 1),
        ("woop", 1),
        ("innerbad", 3),
        ("marh", 3),
        ("marh", 4),
    ]


@pytest.mark.parametrize(
    "sql",
    [
        "WITH FUNCTION answer() RETURNS BIGINT SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT RETURN SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT RETURN 1, SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT RETURN 1",
        "WITH FUNCTION answer() RETURNS BIGINT AS $tag$return 1$tag$ SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT BEGIN RETURN 1 END SELECT 1",
    ],
)
def test_inline_with_function_rejects_malformed_declarations(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_row_expansion_preserves_nested_warning_positions() -> None:
    sql = (
        "SELECT\n"
        "  ROW(\n"
        "    marh(1),\n"
        "    CAST(2 AS bignum)\n"
        "  ).* AS (first, second)"
    )

    result = validate(sql)

    assert result.valid is True
    assert result.statement_count == 1
    assert result.unknown_functions == ["marh"]
    assert result.unknown_types == ["bignum"]
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("marh", 3, 5),
        ("bignum", 4, 15),
    ]


def test_row_expansion_supports_field_and_output_aliases() -> None:
    result = validate("SELECT ROW(1 AS first, 2 second).* AS (left_value, right_value)")

    assert result.valid is True
    assert result.statement_count == 1
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT ROW().*",
        "SELECT ROW(1).* AS ()",
        "SELECT ROW(1).* AS (one,)",
        "SELECT ROW(1).* AS one",
        "SELECT ROW(1 FROM source_table).*",
    ],
)
def test_row_expansion_rejects_malformed_shapes(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_group_by_quantifiers_preserve_warning_positions() -> None:
    sql = "SELECT\n  marh(a)\nFROM t\nGROUP BY DISTINCT marh(a)"

    result = validate(sql)

    assert result.valid is True
    assert result.unknown_functions == ["marh", "marh"]
    assert [(warning.line, warning.column) for warning in result.warnings] == [(2, 3), (4, 19)]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT a, sum(b) FROM t GROUP BY ALL",
        "SELECT a, sum(b) FROM t GROUP BY DISTINCT",
        "SELECT a, sum(b) FROM t GROUP BY ALL HAVING count(*) > 1",
    ],
)
def test_group_by_quantifiers_require_grouping_elements(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_empty_grouping_elements_preserve_warning_positions() -> None:
    result = validate("SELECT 1\nFROM t\nGROUP BY ALL ROLLUP (), marh(a)")

    assert result.valid is True
    assert result.unknown_functions == ["marh"]
    assert [(warning.line, warning.column) for warning in result.warnings] == [(3, 25)]


def test_empty_grouping_elements_do_not_rewrite_functions_or_grouping_sets() -> None:
    function = validate("SELECT rollup()")
    grouping_sets = validate("SELECT 1 GROUP BY GROUPING SETS ()")

    assert function.valid is True
    assert function.unknown_functions == ["rollup"]
    assert grouping_sets.valid is False


def test_at_local_preserves_expression_warning_position() -> None:
    result = validate("SELECT marh(ts) AT LOCAL")

    assert result.valid is True
    assert result.unknown_functions == ["marh"]
    assert [(warning.line, warning.column) for warning in result.warnings] == [(1, 8)]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT current_timestamp AT",
        "SELECT current_timestamp AT UTC",
        "SELECT current_timestamp AT TIME",
    ],
)
def test_at_local_rejects_incomplete_or_unknown_modifiers(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_scalar_values_relation_preserves_nested_warning_positions() -> None:
    sql = (
        "SELECT *\n"
        "FROM LATERAL (\n"
        "  VALUES\n"
        "    marh(1),\n"
        "    CAST(2 AS bignum)\n"
        ")"
    )

    result = validate(sql)

    assert result.valid is True
    assert result.statement_count == 1
    assert result.unknown_functions == ["marh"]
    assert result.unknown_types == ["bignum"]
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("marh", 4, 5),
        ("bignum", 5, 15),
    ]


def test_scalar_values_are_accepted_as_an_insert_query() -> None:
    result = validate("INSERT INTO target @ dev VALUES marh(1), CAST(2 AS bignum)")

    assert result.valid is True, result.error
    assert result.statement_count == 1
    assert result.unknown_functions == ["marh"]
    assert result.unknown_types == ["bignum"]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM LATERAL (VALUES )",
        "SELECT * FROM LATERAL (VALUES 1,)",
    ],
)
def test_scalar_values_relation_rejects_malformed_or_dml_shapes(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) AS ord))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) ord(a, b, c)))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(SELECT * FROM orders) AS ord))",
        (
            "SELECT * FROM TABLE(some_ptf("
            "input => TABLE(orders) AS ord(a, b, c) "
            "PARTITION BY (a, b) PRUNE WHEN EMPTY "
            "ORDER BY (b ASC NULLS LAST)))"
        ),
        (
            "SELECT * FROM TABLE(some_ptf("
            "input1 => TABLE(customers) PARTITION BY nationkey, "
            "input3 => TABLE(lineitem), "
            "input2 => TABLE(nation) PARTITION BY nationkey "
            "COPARTITION (customers, nation)))"
        ),
    ],
)
def test_table_function_table_argument_aliases_and_organization_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error


def test_table_function_organization_preserves_nested_warning_positions() -> None:
    sql = (
        "SELECT *\n"
        "FROM TABLE(some_ptf(\n"
        "  input => TABLE(orders) AS ord\n"
        "    PARTITION BY CAST(marh(a) AS bignum)\n"
        "    KEEP WHEN EMPTY\n"
        "    ORDER BY marh(b) DESC NULLS LAST\n"
        "))"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum"]
    assert result.unknown_functions == ["some_ptf", "marh", "marh"]
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("some_ptf", 2, 12),
        ("marh", 4, 23),
        ("bignum", 4, 34),
        ("marh", 6, 14),
    ]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) AS))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) AS ord()))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) PARTITION a))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) PARTITION BY))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) PRUNE EMPTY))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) ORDER b))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) ORDER BY ()))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) AS ord bogus))",
        "SELECT * FROM TABLE(some_ptf(input => TABLE(orders) COPARTITION(a, b)))",
        (
            "SELECT * FROM TABLE(some_ptf("
            "input => TABLE(orders) PARTITION BY a COPARTITION(a)))"
        ),
        (
            "SELECT * FROM TABLE(some_ptf("
            "input => TABLE(orders) PARTITION BY a COPARTITION(a,)))"
        ),
    ],
)
def test_table_function_table_argument_malformed_neighbors_are_rejected(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 BETWEEN ASYMMETRIC 2 AND 3",
        "SELECT 1 BETWEEN SYMMETRIC 2 AND 3",
        "SELECT 1 NOT BETWEEN SYMMETRIC 2 AND 3",
        "SELECT trim(BOTH FROM ' abc ')",
        "SELECT trim(LEADING FROM ' abc ')",
        "SELECT trim(TRAILING FROM ' abc ')",
        "SELECT U&'' UESCAPE ')'",
        "SELECT U&'abc#0041' UESCAPE '#'",
        "SELECT transform(ARRAY[1], () -> 42)",
        "SELECT ('a').trim()",
        "SELECT bigint::parse(value => '42')",
        "SELECT RUNNING LAST(x, 1)",
        "SELECT FINAL FIRST(x, 1)",
        "SELECT col1 = ALL (VALUES ROW(1), ROW(2))",
        "SELECT 1_000_000 + 0xCA_FE",
    ],
)
def test_trino_expression_extensions_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error


@pytest.mark.parametrize(
    ("sql", "name"),
    [
        ("SELECT ('a').marh()", "marh"),
        ("SELECT bigint::marh(value => 42)", "marh"),
        ("SELECT RUNNING marh(x, 1)", "marh"),
    ],
)
def test_trino_expression_extensions_preserve_function_warning_positions(
    sql: str, name: str
) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_functions == [name]
    assert [(warning.line, warning.column) for warning in result.warnings] == [
        (1, sql.index(name) + 1)
    ]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 BETWEEN SYMMETRIC AND 3",
        "SELECT trim(BOTH FROM)",
        "SELECT transform(ARRAY[1], () ->)",
        "SELECT U&'hello\\8Bd5' UESCAPE '%%'",
        "SELECT U&'hello\\8Bd5' UESCAPE ''",
        "SELECT U&'hello\\8Bd5' UESCAPE '1'",
        "SELECT U&'hello\\6dB\\8Bd5'",
        "SELECT bigint::(value => 42)",
    ],
)
def test_trino_expression_extension_malformed_neighbors_are_rejected(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


def test_trino_statement_rejects_unbalanced_groups() -> None:
    result = validate("ALTER BRANCH b SET RETENTION (3")
    assert result.valid is False


def test_prepare_normalization_preserves_warning_columns() -> None:
    result = validate("PREPARE p FROM SELECT marh(1)")
    assert result.valid is True
    assert result.unknown_functions == ["marh"]
    assert result.warnings[0].column == 23


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM (VALUES 'one', 'two') AS t(value)",
        "SELECT * FROM (VALUES VARCHAR 'value') AS t(value)",
        "SELECT * FROM (VALUES ARRAY[1, 2]) AS t(value)",
        "SELECT * FROM (VALUES map_from_entries(ARRAY[('one', 1)])) AS t(value)",
    ],
)
def test_tokenized_values_compatibility_forms_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 1


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT IPADDRESS '10.0.0.1', marh(1)",
        "SELECT * FROM customer FOR VERSION AS OF 'audit' WHERE marh(custkey)",
        "SELECT * FROM customer FOR TIMESTAMP AS OF DATE '2022-03-23' WHERE marh(custkey)",
        "SELECT CAST(marh(1) AS ARRAY (BIGINT))",
    ],
)
def test_tokenized_compatibility_forms_preserve_warning_positions(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_functions == ["marh"]
    assert result.warnings[0].column == sql.index("marh") + 1


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 AS x UNION CORRESPONDING SELECT 2 AS x",
        "SELECT 1 AS x UNION ALL CORRESPONDING BY (x) SELECT 2 AS x",
        "SELECT 1 AS x INTERSECT CORRESPONDING BY (x) SELECT 2 AS x",
        "SELECT 1 AS x EXCEPT CORRESPONDING BY (x) SELECT 2 AS x",
    ],
)
def test_corresponding_set_operations_validate(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 UNION CORRESPONDING BY () SELECT 2",
        "SELECT 1 UNION CORRESPONDING BY (x,) SELECT 2",
        "SELECT 1 UNION CORRESPONDING BY x SELECT 2",
    ],
)
def test_malformed_corresponding_set_operations_are_rejected(sql: str) -> None:
    assert validate(sql).valid is False


def test_corresponding_set_operations_preserve_warning_positions() -> None:
    sql = "SELECT marh(1) AS x UNION CORRESPONDING BY (x) SELECT 2 AS x"
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_functions == ["marh"]
    assert result.warnings[0].column == sql.index("marh") + 1


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM sales PIVOT (sum(amount) FOR month IN (1 AS jan) GROUP BY region)",
        "SELECT * FROM sales PIVOT (sum(marh(amount)) FOR month IN (1 AS jan) GROUP BY region)",
    ],
)
def test_pivot_group_by_validates(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    if "marh" in sql:
        assert result.unknown_functions == ["marh"]
        assert result.warnings[0].column == sql.index("marh") + 1


def test_pivot_group_by_preserves_grouping_warning_positions() -> None:
    from trino_sql_validator import FunctionWarning, TypeWarning

    sql = (
        "SELECT *\n"
        "FROM sales PIVOT (\n"
        "  sum(amount) FOR month IN (1 AS jan)\n"
        "  GROUP BY CAST(marh(region) AS bignum)\n"
        ")"
    )
    result = validate(sql)

    assert result.valid is True, result.error
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("marh", 4, 17),
        ("bignum", 4, 33),
    ]
    assert isinstance(result.warnings[0], FunctionWarning)
    assert isinstance(result.warnings[1], TypeWarning)


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM sales PIVOT (sum(amount) FOR month IN (1 AS jan) GROUP region)",
        "SELECT * FROM sales PIVOT (sum(amount) FOR month IN (1 AS jan) GROUP BY)",
        "SELECT * FROM sales PIVOT (sum(amount) GROUP BY region FOR month IN (1 AS jan))",
    ],
)
def test_malformed_pivot_group_by_is_rejected(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM trades CROSS JOIN NEAREST (FROM quotes MATCH quotes.ts <= trades.ts)",
        "SELECT * FROM trades, NEAREST (FROM quotes WHERE quotes.symbol = trades.symbol MATCH quotes.ts <= trades.ts)",
        "SELECT * FROM trades LEFT JOIN NEAREST (FROM quotes WHERE quotes.symbol = trades.symbol MATCH quotes.ts <= trades.ts) ON TRUE",
    ],
)
def test_nearest_relations_validate(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM trades CROSS JOIN NEAREST (quotes MATCH quotes.ts <= trades.ts)",
        "SELECT * FROM trades CROSS JOIN NEAREST (FROM quotes WHERE quotes.symbol = trades.symbol)",
        "SELECT * FROM trades CROSS JOIN NEAREST (FROM quotes WHERE MATCH quotes.ts <= trades.ts)",
        "SELECT * FROM trades CROSS JOIN NEAREST (FROM quotes MATCH)",
    ],
)
def test_malformed_nearest_relations_are_rejected(sql: str) -> None:
    assert validate(sql).valid is False


def test_nearest_relations_preserve_warning_positions() -> None:
    sql = "SELECT * FROM trades CROSS JOIN NEAREST (FROM quotes WHERE marh(quotes.symbol) = trades.symbol MATCH marh(quotes.ts) <= trades.ts)"
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.unknown_functions == ["marh", "marh"]
    assert [warning.column for warning in result.warnings] == [
        offset + 1 for offset in (sql.index("marh"), sql.rindex("marh"))
    ]


def test_with_session_preserves_property_value_warning_positions() -> None:
    from trino_sql_validator import FunctionWarning, TypeWarning

    sql = "WITH SESSION\n  example.setting = marh(CAST(1 AS bignum))\nSELECT 1"
    result = validate(sql)

    assert result.valid is True, result.error
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("marh", 2, 21),
        ("bignum", 2, 36),
    ]
    assert isinstance(result.warnings[0], FunctionWarning)
    assert isinstance(result.warnings[1], TypeWarning)


@pytest.mark.parametrize(
    "sql",
    [
        "WITH RECURSIVE h(id, path) AS (SELECT 1, CAST(ARRAY[] AS ARRAY(VARCHAR)) UNION ALL SELECT id, CAST(ARRAY[id] AS ARRAY(VARCHAR)) FROM h) SELECT * FROM h",
        "SELECT client_order_id, payment_type_id FROM (SELECT top.client_order_id, top.payment_type_id, ROW_NUMBER() OVER (PARTITION BY top.client_order_id ORDER BY top.start_time DESC) AS rn FROM dds_data.his AS top) top WHERE rn = 1",
        "SELECT CAST(SPLIT(value, ',') AS ARRAY (BIGINT)) FROM source",
        "SELECT id, path, level FROM (WITH RECURSIVE h(id, path) AS (SELECT 1, CAST(ARRAY[1] AS array(bigint)) UNION ALL SELECT id, CAST((path || ARRAY[id]) AS array(bigint)) FROM h) SELECT *, CARDINALITY(path) - 1 AS level FROM h) AS tm"
    ],
)
def test_reported_recursive_trino_queries_validate(sql: str) -> None:
    result = validate(sql)
    assert result.valid is True
    assert result.statement_count == 1


@pytest.mark.parametrize("sql", ["SELECT a FROM WHERE", "SELECT a FROM GROUP", "SELECT a FROM ORDER"])
def test_trino_rejects_clause_keyword_as_from_relation(sql: str) -> None:
    result = validate(sql)
    assert result.valid is False


def test_trino_allows_quoted_clause_keyword_as_table_name() -> None:
    result = validate('SELECT a FROM "where"')
    assert result.valid is True


def test_transformed_recursive_script_validates() -> None:
    result = validate_file(FIXTURES / "trino_recursive_transformed.sql")
    assert result.valid is True
    assert result.statement_count == 4


def test_sqlparser_merge_fixture_validates() -> None:
    result = validate_file(FIXTURES / "sqlparser_merge_example.sql")
    assert result.valid is True
    assert result.statement_count == 1


def test_nested_row_fixture_validates() -> None:
    result = validate_file(FIXTURES / "trino_reports_tests_schema.sql")
    assert result.valid is True
    assert result.statement_count == 6
    assert result.error is None
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "INSERT INTO customer @ dev (id) VALUES (1)",
        "DELETE FROM customer @ dev WHERE id = 1",
        "UPDATE catalog.schema.customer @ dev SET id = 1",
        (
            "MERGE INTO target @ dev USING source ON target.id = source.id "
            "WHEN MATCHED THEN DELETE"
        ),
    ],
)
def test_iceberg_dml_branch_references_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 1


@pytest.mark.parametrize("sql", ["@select", "SELECT @branch", "DELETE @ branch"])
def test_arbitrary_at_words_are_not_removed(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "UPDATE customer @ dev SET score = marh(1)",
        "SELECT '@branch', marh(1)",
        "SELECT 1 -- @branch\n, marh(1)",
    ],
)
def test_branch_normalization_preserves_warning_position(sql: str) -> None:
    result = validate(sql)
    prefix = sql[: sql.index("marh")]

    assert result.valid is True, result.error
    assert result.unknown_functions == ["marh"]
    assert result.warnings[0].line == prefix.count("\n") + 1
    assert result.warnings[0].column == len(prefix.rsplit("\n", maxsplit=1)[-1]) + 1


@pytest.mark.parametrize("identifier", ["имя", "foo$bar"])
def test_trino_rejects_non_ascii_or_dollar_unquoted_identifier(identifier: str) -> None:
    assert validate(f"SELECT {identifier}").valid is False
    assert validate(f'SELECT "{identifier}"').valid is True


@pytest.mark.parametrize("sql", ["SELECT 1x FROM dual", 'SELECT ""', 'SELECT * FROM ""'])
def test_trino_rejects_invalid_identifier_shapes(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE TABLE foo () AS (VALUES 1)",
        "SELECT count(DISTINCT *) FROM (VALUES 1)",
    ],
)
def test_trino_rejects_empty_ctas_columns_and_distinct_wildcard_count(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize("sql", ["SELECT 1 x FROM dual", 'SELECT "1x"', "SELECT ''"])
def test_valid_literals_aliases_and_quoted_identifiers_remain_valid(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 0X123_ABC_DEF",
        "SELECT -0x123_abc_def",
        "SELECT 0O012_345",
        "SELECT -0o012_345",
        "SELECT 0B110_010",
        "SELECT -0b110_010",
    ],
)
def test_trino_accepts_non_decimal_integer_literals(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize("sql", ["SELECT 0X123_G", "SELECT 0O018", "SELECT 0B102"])
def test_trino_rejects_malformed_non_decimal_integer_literals(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE CATALOG IF NOT EXISTS hive USING hive WITH (\"hive.metastore.uri\" = 'thrift://host:9083')",
        "CREATE CATALOG test USING conn COMMENT 'awesome' AUTHORIZATION ROLE dragon WITH (\"a\" = 'apple', \"b\" = 123)",
        "DROP CATALOG IF EXISTS hive RESTRICT",
        "ALTER TABLE orders SET PROPERTIES format = 'ORC', partitioned_by = ARRAY['ds']",
        "ALTER MATERIALIZED VIEW daily_orders SET PROPERTIES refresh_interval = '1h'",
        "SET PATH analytics, hive.default",
        "SET SESSION AUTHORIZATION 'analyst'",
        'SET SESSION AUTHORIZATION "null"',
        "EXPLAIN ANALYZE VERBOSE SELECT * FROM orders",
        "SHOW CATALOGS LIKE '%$_%' ESCAPE '$'",
        "SHOW SCHEMAS IN hive LIKE '%$_%' ESCAPE '$'",
        "SHOW TABLES FROM hive.default LIKE '%$_%' ESCAPE '$'",
        "SHOW COLUMNS FROM hive.default.orders LIKE '%$_%' ESCAPE '$'",
        "SHOW FUNCTIONS FROM hive.default LIKE '%$_%' ESCAPE '$'",
        "SHOW SESSION LIKE '%$_%' ESCAPE '$'",
        "CREATE ROLE role1 WITH ADMIN CURRENT_USER IN hive",
        "DROP ROLE IF EXISTS role1 IN hive",
        "SET ROLE role1 IN hive",
        "GRANT role1 TO USER alice WITH ADMIN OPTION GRANTED BY CURRENT_ROLE IN hive",
        "GRANT SELECT ON TABLE hive.default.orders TO ROLE analyst WITH GRANT OPTION",
        "REVOKE GRANT OPTION FOR SELECT ON TABLE hive.default.orders FROM ROLE analyst",
        "DENY DELETE ON hive.default.orders TO USER alice",
        "ALTER TABLE orders RENAME COLUMN IF EXISTS payload.old_name TO new_name",
        "ALTER TABLE orders ADD COLUMN IF NOT EXISTS payload.item BIGINT LAST",
        "ALTER TABLE orders DROP COLUMN IF EXISTS payload.item",
        "ALTER TABLE orders ALTER COLUMN payload.item SET DATA TYPE VARCHAR",
    ],
)
def test_current_trino_statement_forms_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 1


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE TABLE IF NOT EXISTS bar (c VARCHAR WITH (nullable = true, compression = 'LZ4'))",
        "CREATE TABLE bar (LIKE source INCLUDING PROPERTIES)",
        "CREATE TABLE bar (c VARCHAR, LIKE source EXCLUDING PROPERTIES) COMMENT 'copy'",
        "CREATE TABLE foo(x, y) AS SELECT a, b FROM source",
        "CREATE OR REPLACE TABLE foo(x) AS SELECT a FROM source WITH DATA",
        "CREATE TABLE IF NOT EXISTS foo(x) AS SELECT a FROM source WITH NO DATA",
        "ANALYZE foo WITH (sample = 10, columns = ARRAY['a', 'b'])",
        "CREATE VIEW report COMMENT 'report' SECURITY DEFINER AS SELECT * FROM source",
        "CREATE VIEW report SECURITY INVOKER WITH (owner = 'analytics') AS SELECT 1",
        "SELECT ALL, SOME, ANY FROM source",
    ],
)
def test_trino_483_statement_extensions_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 1


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE TABLE bar (c VARCHAR WITH ())",
        "CREATE TABLE bar (c VARCHAR WITH (nullable))",
        "CREATE TABLE bar (LIKE source INCLUDING)",
        "CREATE TABLE bar (LIKE source SOMETIMES PROPERTIES)",
        "CREATE OR REPLACE TABLE IF NOT EXISTS foo AS SELECT 1",
        "CREATE TABLE foo(x,) AS SELECT 1",
        "CREATE TABLE foo(x) AS SELECT 1 WITH NO DATA trailing",
        "ANALYZE foo WITH ()",
        "ANALYZE foo WITH (sample =)",
        "CREATE VIEW report COMMENT 1 AS SELECT 1",
        "CREATE VIEW report SECURITY OWNER AS SELECT 1",
        "CREATE VIEW report SECURITY DEFINER COMMENT 'late' AS SELECT 1",
    ],
)
def test_trino_483_statement_extensions_reject_malformed_neighbors(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0


def test_statement_extension_metadata_preserves_warning_locations() -> None:
    sql = (
        "CREATE TABLE report (\n"
        "  payload bignum WITH (computed = marh(1))\n"
        ");\n"
        "ANALYZE report WITH (computed = marh(2));\n"
        "CREATE TABLE copy(x) AS SELECT marh(CAST(3 AS bignum)) WITH NO DATA"
    )

    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 3
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("bignum", 2, 11),
        ("marh", 2, 35),
        ("marh", 4, 33),
        ("marh", 5, 32),
        ("bignum", 5, 47),
    ]


def test_custom_statement_metadata_preserves_nested_warnings() -> None:
    sql = (
        "ALTER TABLE report SET PROPERTIES computed = marh(1);\n"
        "ALTER MATERIALIZED VIEW report EXECUTE refresh(value => zoop(2)) "
        "WHERE quux(id);\n"
        "DESCRIBE OUTPUT (SELECT marh(CAST(3 AS bignum)))"
    )

    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 3
    assert [(warning.name, warning.line, warning.column) for warning in result.warnings] == [
        ("marh", 1, 46),
        ("zoop", 2, 57),
        ("quux", 2, 72),
        ("marh", 3, 25),
        ("bignum", 3, 40),
    ]


@pytest.mark.parametrize(
    ("sql", "line", "column"),
    [
        ("CREATE VIEW report\nSECURITY OWNER AS SELECT 1", 2, 10),
        ("ANALYZE report WITH (\n  sample =\n)", 2, 10),
        ("DESCRIBE OUTPUT (\n  SELECT FROM\n)", 3, 1),
    ],
)
def test_custom_statement_errors_keep_multiline_locations(
    sql: str, line: int, column: int
) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert (result.error.line, result.error.column) == (line, column)


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE OR REPLACE CATALOG hive USING hive",
        "CREATE CATALOG hive USING hive (x = 1)",
        "CREATE OR REPLACE BRANCH IF NOT EXISTS audit IN TABLE orders",
        "DROP BRANCH audit",
        "ALTER TABLE orders SET PROPERTIES (format = 'ORC')",
        "ALTER MATERIALIZED VIEW daily_orders SET PROPERTIES (refresh_interval = '1h')",
        "ALTER VIEW daily_orders SET PROPERTIES refresh_interval = '1h'",
        "SET PATH one.too.many, qualifiers",
        "SET SESSION AUTHORIZATION null",
        "EXPLAIN VERBOSE SELECT * FROM orders",
        "SHOW SESSION LIKE '%$_%' ESCAPE",
        "SHOW COLUMNS orders",
        "CREATE ROLE role1 WITH ADMIN",
        "DROP ROLE role1 IN",
        "SET ROLE ALL trailing",
        "GRANT role1 TO USER alice WITH GRANT OPTION",
        "GRANT ALL ON TABLE orders TO ROLE analyst",
        "REVOKE SELECT ON TABLE orders TO ROLE analyst",
        "DENY SELECT TABLE orders TO ROLE analyst",
        "ALTER TABLE orders RENAME COLUMN payload.old_name new_name",
        "ALTER TABLE orders ADD COLUMN IF EXISTS payload.item BIGINT",
        "ALTER TABLE orders ADD COLUMN payload.item BIGINT AFTER",
        "ALTER TABLE orders SET AUTHORIZATION USER ROLE alice",
    ],
)
def test_trino_statement_false_accepts_are_rejected(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "select *\nfrom x\nwhere from",
        "CREATE TABLE foo ",
        "SELECT a FROM a AS x TABLESAMPLE x ",
        "SELECT foo(*) filter (",
        "SELECT (DATE '2022-10-10', DOUBLE 12.0)",
        "VALUES(DATE 2)",
    ],
)
def test_trino_v483_syntax_false_accepts_are_rejected(sql: str) -> None:
    assert validate(sql).valid is False
