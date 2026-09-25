import pytest
from trino_sql_validator import AliasWarning, validate


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 == 1",
        "SELECT json_extract(payload, $.page) FROM events",
        "SELECT X'ABC'",
        ";",
        "SELECT 1;;",
        "SELECT 1; ; SELECT 2",
    ],
)
def test_trino_rejects_lexical_and_empty_statement_gaps(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.warnings == ()


@pytest.mark.parametrize("dialect", ["generic", "hive"])
def test_double_equals_restriction_is_trino_only(dialect: str) -> None:
    assert validate("SELECT 1 == 1", dialect=dialect).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "",
        "-- comment only",
        "SELECT 1;",
        "SELECT 1; SELECT 2;",
        "SELECT ';'",
        "CREATE FUNCTION f(x BIGINT) RETURNS BIGINT BEGIN RETURN x + 1; END;",
    ],
)
def test_valid_statement_boundaries_remain_supported(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT if(true)",
        "SELECT if(true, 1, 2, 3)",
        "SELECT nullif(1)",
        "SELECT nullif(1, 2, 3)",
        "SELECT coalesce()",
        "SELECT coalesce(1)",
        "SELECT try(1, 2)",
        "SELECT format('%s')",
        "SELECT if(DISTINCT true, 1)",
        "SELECT coalesce(1, 2) OVER ()",
    ],
)
def test_parser_special_functions_follow_trino_parse_contract(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT if(true, 1)",
        "SELECT if(false, 1, 2)",
        "SELECT nullif(1, 2)",
        "SELECT coalesce(1, 2)",
        "SELECT try(1 / 0)",
        "SELECT format('%s', 1)",
    ],
)
def test_valid_parser_special_functions_remain_supported(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT * FROM select",
        "CREATE TABLE where (id integer)",
        "CREATE TABLE example (order integer)",
        "SELECT from FROM orders",
        "SELECT source.from FROM source",
    ],
)
def test_reserved_words_are_invalid_in_identifier_roles(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        'SELECT * FROM "select"',
        'CREATE TABLE "where" ("order" integer)',
        'SELECT "from" FROM orders',
        'SELECT source."from" FROM source',
        'SELECT "from"."where" FROM "select" AS "from"',
    ],
)
def test_quoted_reserved_words_remain_valid_identifiers(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CURRENT_DATE()",
        "SELECT CURRENT_TIME()",
        "SELECT CURRENT_TIMESTAMP(3, 4)",
        "SELECT LOCALTIME(x)",
        "SELECT LOCALTIMESTAMP(1.5)",
    ],
)
def test_invalid_current_value_syntax_is_rejected(sql: str) -> None:
    assert validate(sql).valid is False


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CURRENT_DATE",
        "SELECT CURRENT_TIME(3)",
        "SELECT CURRENT_TIMESTAMP",
        "SELECT LOCALTIME(2)",
        "SELECT LOCALTIMESTAMP",
        "SELECT CURRENT_SCHEMA, CURRENT_PATH",
    ],
)
def test_valid_current_values_remain_supported(sql: str) -> None:
    assert validate(sql).valid is True


def test_open_pattern_quantifiers_are_valid() -> None:
    for quantifier in ["{,}", "{,}?"]:
        sql = (
            "SELECT * FROM orders MATCH_RECOGNIZE "
            f"(PATTERN (a{quantifier}) DEFINE a AS true)"
        )
        assert validate(sql).valid is True


def test_over_is_a_contextual_projection_alias_before_from() -> None:
    sql = "SELECT row_number() OVER FROM orders"
    result = validate(sql)

    assert result.valid is True
    assert result.warnings == (AliasWarning("over", line=1, column=21),)


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1,\r\nFROM orders",
        "SELECT JSON_VALUE(payload) /* path is required */ FROM events",
        "SELECT * FROM orders\nFETCH NEXT 5 ONLY",
    ],
)
def test_located_guards_report_source_positions(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert result.error.line is not None
    assert result.error.column is not None


@pytest.mark.parametrize(
    "sql",
    [
        "WITH a AS (SELECT 1), b (SELECT 2) SELECT * FROM b",
        "WITH a AS (SELECT 1), b(x) (SELECT 2) SELECT * FROM b",
        "WITH a AS (SELECT 1), b AS (SELECT 2), c (SELECT 3) SELECT * FROM c",
        "WITH a AS (WITH b (SELECT 1) SELECT * FROM b) SELECT * FROM a",
        'WITH x("select", "where") (SELECT 1, 2) SELECT * FROM x',
        "SELECT RUNNING if(true, 1)",
        'SELECT RUNNING "if"(true, 1)',
        "SELECT FINAL coalesce(1, 2)",
        "SELECT if(t.*, 1) FROM t",
        "SELECT nullif(t.*, 1) FROM t",
        "SELECT coalesce(t.*, 1) FROM t",
        "SELECT try(t.*, 1) FROM t",
        "SELECT format(t.*, 1) FROM t",
        "CREATE VIEW select AS SELECT 1",
        "DROP VIEW select",
        "CREATE SCHEMA select",
        "ALTER TABLE select RENAME TO x",
        "GRANT SELECT ON TABLE select TO ROLE analyst",
        "PREPARE select FROM SELECT 1",
        "EXECUTE select",
        "DEALLOCATE PREPARE select",
        "CREATE FUNCTION f(select BIGINT) RETURNS BIGINT RETURN 1",
        "CREATE FUNCTION f(x ROW(select BIGINT)) RETURNS BIGINT RETURN 1",
        "SELECT CAST(ROW(1) AS ROW(select BIGINT))",
        "SELECT * FROM a JOIN b USING (select)",
        "SELECT where(1)",
        "CALL select()",
    ],
)
def test_post_review_syntax_gaps_are_rejected_with_locations(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.line is not None
    assert result.error.column is not None
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "WITH a AS (SELECT 1), b AS (SELECT 2), c(x) AS (SELECT 3) SELECT * FROM c",
        (
            "WITH outer_cte AS (WITH inner_cte AS (SELECT 1) SELECT * FROM inner_cte) "
            "SELECT * FROM outer_cte"
        ),
        'WITH x("select", "where") AS (SELECT 1, 2) SELECT * FROM x',
        "SELECT RUNNING sum(x) FROM t",
        "SELECT FINAL first(x) FROM t",
        "SELECT try(t.*) FROM t",
        'CREATE VIEW "select" AS SELECT 1',
        'DROP VIEW "select"',
        'CREATE SCHEMA "select"',
        'ALTER TABLE "select" RENAME TO x',
        'GRANT SELECT ON TABLE "select" TO ROLE analyst',
        'PREPARE "select" FROM SELECT 1',
        'EXECUTE "select"',
        'DEALLOCATE PREPARE "select"',
        'CREATE FUNCTION "select"("where" BIGINT) RETURNS BIGINT RETURN "where"',
        (
            'CREATE FUNCTION f(x ROW("select" BIGINT)) RETURNS ROW("where" BIGINT) '
            'RETURN CAST(ROW(1) AS ROW("where" BIGINT))'
        ),
        'SELECT CAST(ROW(1) AS ROW("select" BIGINT))',
        'SELECT * FROM a JOIN b USING ("select")',
        'SELECT "where"(1)',
        'CALL "select"()',
    ],
)
def test_post_review_valid_neighbors_remain_supported(sql: str) -> None:
    assert validate(sql).valid is True


@pytest.mark.parametrize(
    "word", ["after", "all", "one", "pattern", "subset", "define"]
)
def test_match_recognize_non_reserved_words_remain_valid_in_measures(word: str) -> None:
    sql = (
        "SELECT * FROM t MATCH_RECOGNIZE "
        f"(MEASURES {word} + 1 AS m PATTERN (A) DEFINE A AS true)"
    )

    assert validate(sql).valid is True


def test_match_recognize_multiple_measure_aliases_require_as_independently() -> None:
    valid = validate(
        "SELECT * FROM t MATCH_RECOGNIZE "
        "(MEASURES after + 1 AS first_value, A.x AS second_value "
        "PATTERN (A) DEFINE A AS true)"
    )
    missing_as = validate(
        "SELECT * FROM t MATCH_RECOGNIZE "
        "(MEASURES A.x AS first_value, A.y second_value "
        "PATTERN (A) DEFINE A AS true)"
    )

    assert valid.valid is True
    assert missing_as.valid is False
    assert missing_as.error is not None
    assert missing_as.error.line is not None
    assert missing_as.error.column is not None


@pytest.mark.parametrize("mode", ["RUNNING", "FINAL"])
@pytest.mark.parametrize(
    "call",
    [
        "if(true, 1)",
        "nullif(1, 2)",
        "coalesce(1, 2)",
        "try(1)",
        "format('%s', 1)",
    ],
)
def test_processing_modes_are_rejected_for_every_parser_special_function(
    mode: str, call: str
) -> None:
    result = validate(f"SELECT {mode} {call}")

    assert result.valid is False
    assert result.error is not None
    assert result.error.line is not None
    assert result.error.column is not None
