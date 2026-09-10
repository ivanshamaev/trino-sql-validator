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


@pytest.mark.parametrize(
    "sql",
    [
        "WITH FUNCTION answer() RETURNS BIGINT SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT RETURN SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT RETURN 1, SELECT 1",
        "WITH FUNCTION answer() RETURNS BIGINT RETURN 1",
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


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM LATERAL (VALUES )",
        "SELECT * FROM LATERAL (VALUES 1,)",
        "INSERT INTO target VALUES 1",
    ],
)
def test_scalar_values_relation_rejects_malformed_or_dml_shapes(sql: str) -> None:
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
        "EXPLAIN ANALYZE VERBOSE SELECT * FROM orders",
        "SHOW CATALOGS LIKE '%$_%' ESCAPE '$'",
        "SHOW SCHEMAS IN hive LIKE '%$_%' ESCAPE '$'",
        "SHOW TABLES FROM hive.default LIKE '%$_%' ESCAPE '$'",
        "SHOW COLUMNS FROM hive.default.orders LIKE '%$_%' ESCAPE '$'",
        "SHOW FUNCTIONS FROM hive.default LIKE '%$_%' ESCAPE '$'",
        "SHOW SESSION LIKE '%$_%' ESCAPE '$'",
    ],
)
def test_current_trino_statement_forms_validate(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 1


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
