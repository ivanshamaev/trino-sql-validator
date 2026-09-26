"""Trino syntax cases adapted from SQLGlot's MIT-licensed Trino tests.

Source revision: d01e9461a7a3fcbe50a965c7a2ddf55d41aca97d
Source file SHA-256: ccc5d7450c58910676c2ed999c7a12eaee8186637e09adda8a1ace3a65ecaa9f
Source: https://github.com/tobymao/sqlglot/blob/main/tests/dialects/test_trino.py
License: https://github.com/tobymao/sqlglot/blob/main/LICENSE

Only syntax families absent from this project's tests were selected. Every
positive and negative statement below was also checked with Trino 483's native
SqlParser. Expression-only SQLGlot cases are wrapped in SELECT because this
project validates complete statements.
"""

import pytest
from trino_sql_validator import validate

SQLGLOT_TRINO_POSITIVE_CASES = {
    "fetch_first_singular_row": "SELECT * FROM t ORDER BY x FETCH FIRST 1 ROW ONLY",
    "fetch_next_singular_row_with_ties": (
        "SELECT * FROM t ORDER BY x FETCH NEXT 1 ROW WITH TIES"
    ),
    "fetch_implicit_quantity_singular_row": (
        "SELECT * FROM t ORDER BY x FETCH FIRST ROW ONLY"
    ),
    "concat_ws_array_with_null": "SELECT CONCAT_WS('-', ARRAY['a', NULL, 'b'])",
    "concat_ws_null_array": "SELECT CONCAT_WS('-', CAST(NULL AS ARRAY(VARCHAR)))",
    "json_query_omit_quotes": (
        "SELECT JSON_QUERY(m.properties, 'lax $.area' OMIT QUOTES NULL ON ERROR)"
    ),
    "json_query_keep_quotes": (
        "SELECT JSON_QUERY(description, 'strict $.comment' KEEP QUOTES)"
    ),
    "json_query_omit_quotes_on_scalar_string": (
        "SELECT JSON_QUERY(description, "
        "'strict $.comment' OMIT QUOTES ON SCALAR STRING)"
    ),
    "json_query_wrapper_and_keep_quotes": (
        "SELECT JSON_QUERY(content, "
        "'strict $.HY.*' WITH UNCONDITIONAL WRAPPER KEEP QUOTES)"
    ),
    "timestamp_literal_short_offset": "SELECT TIMESTAMP '2012-10-31 01:00 -2'",
    "time_literal_offset": "SELECT TIME '01:02:03.456 -08:00'",
    "listagg_distinct": (
        "SELECT LISTAGG(DISTINCT col, ',') "
        "WITHIN GROUP (ORDER BY col ASC) FROM tbl"
    ),
    "listagg_overflow_error": (
        "SELECT LISTAGG(col, '; ' ON OVERFLOW ERROR) "
        "WITHIN GROUP (ORDER BY col ASC) FROM tbl"
    ),
    "listagg_overflow_truncate_count": (
        "SELECT LISTAGG(col, '; ' ON OVERFLOW TRUNCATE WITH COUNT) "
        "WITHIN GROUP (ORDER BY col ASC) FROM tbl"
    ),
    "listagg_overflow_custom_filler_without_count": (
        "SELECT LISTAGG(col, '; ' ON OVERFLOW TRUNCATE '...' WITHOUT COUNT) "
        "WITHIN GROUP (ORDER BY col ASC) FROM tbl"
    ),
    "trim_both_characters": "SELECT TRIM(BOTH '$' FROM '$var$')",
    "trim_trailing_expression": (
        "SELECT TRIM(TRAILING 'ER' FROM UPPER('worker'))"
    ),
    "array_first_lambda": (
        "SELECT ARRAY_FIRST(ARRAY['a', 'b'], x -> x = 'b') FROM tbl"
    ),
    "inline_function_called_on_null_input": (
        "WITH FUNCTION f() RETURNS INTEGER CALLED ON NULL INPUT RETURN 1 SELECT F()"
    ),
    "inline_function_multi_name_declare": (
        "WITH FUNCTION f() RETURNS INTEGER BEGIN "
        "DECLARE first_name, last_name, middle_name VARCHAR(25); "
        "RETURN 1; END SELECT F()"
    ),
    "inline_function_nested_begin": (
        "WITH FUNCTION f() RETURNS INTEGER BEGIN "
        "DECLARE x INTEGER DEFAULT 1; BEGIN SET x = x + 1; END; "
        "RETURN x; END SELECT F()"
    ),
    "inline_function_nested_if": (
        "WITH FUNCTION f(a INTEGER, b INTEGER) RETURNS VARCHAR BEGIN "
        "IF a = 0 THEN IF b = 0 THEN RETURN 'both zero'; "
        "ELSE RETURN 'a zero'; END IF; ELSE RETURN 'a nonzero'; END IF; "
        "RETURN 'unreachable'; END SELECT F(1, 2)"
    ),
    "inline_function_nested_case": (
        "WITH FUNCTION nested_case(a BIGINT, b BIGINT) RETURNS VARCHAR BEGIN "
        "DECLARE result VARCHAR; CASE WHEN a = 0 THEN CASE b "
        "WHEN 0 THEN SET result = 'a0b0'; ELSE SET result = 'a0bN'; "
        "END CASE; ELSE SET result = 'aN'; END CASE; RETURN result; END "
        "SELECT NESTED_CASE(0, 0)"
    ),
    "inline_function_iterate_repeat": (
        "WITH FUNCTION iter_count() RETURNS BIGINT BEGIN "
        "DECLARE a BIGINT DEFAULT 0; DECLARE b BIGINT DEFAULT 0; "
        "top: REPEAT SET a = a + 1; IF a <= 3 THEN ITERATE top; END IF; "
        "SET b = b + 1; UNTIL a >= 10 END REPEAT; RETURN b; END "
        "SELECT ITER_COUNT()"
    ),
    "inline_function_iterate_keyword_label": (
        "WITH FUNCTION label_test(n BIGINT) RETURNS BIGINT BEGIN "
        "DECLARE i BIGINT DEFAULT 0; iterate: LOOP "
        "IF i >= n THEN LEAVE iterate; END IF; SET i = i + 1; END LOOP; "
        "RETURN i; END SELECT LABEL_TEST(5)"
    ),
    "inline_function_leave_keyword_label": (
        "WITH FUNCTION label_test2(n BIGINT) RETURNS BIGINT BEGIN "
        "DECLARE i BIGINT DEFAULT 0; leave: LOOP "
        "IF i >= n THEN LEAVE leave; END IF; SET i = i + 1; END LOOP; "
        "RETURN i; END SELECT LABEL_TEST2(5)"
    ),
    "inline_function_set_keyword_label": (
        "WITH FUNCTION label_test3(n BIGINT) RETURNS BIGINT BEGIN "
        "DECLARE i BIGINT DEFAULT 0; set: LOOP "
        "IF i >= n THEN LEAVE set; END IF; SET i = i + 1; END LOOP; "
        "RETURN i; END SELECT LABEL_TEST3(5)"
    ),
}


@pytest.mark.parametrize(
    "sql",
    SQLGLOT_TRINO_POSITIVE_CASES.values(),
    ids=SQLGLOT_TRINO_POSITIVE_CASES.keys(),
)
def test_sqlglot_trino_positive_syntax_verified_by_native_trino(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.statement_count == 1


SQLGLOT_RELATED_NATIVE_NEGATIVE_CASES = {
    "json_query_incomplete_on_scalar_string": (
        "SELECT JSON_QUERY(x, '$' KEEP QUOTES ON SCALAR)"
    ),
    "json_query_duplicate_quotes_clause": (
        "SELECT JSON_QUERY(x, '$' KEEP QUOTES OMIT QUOTES)"
    ),
    "routine_repeated_end_label": (
        "WITH FUNCTION f(n BIGINT) RETURNS BIGINT BEGIN "
        "abc: WHILE n > 0 DO SET n = n - 1; END WHILE abc; "
        "RETURN n; END SELECT F(1)"
    ),
    "routine_label_on_non_loop_statement": (
        "WITH FUNCTION f() RETURNS INTEGER BEGIN bad: RETURN 1; END SELECT F()"
    ),
    "single_quoted_alter_property_name": (
        "ALTER TABLE people SET PROPERTIES foo = 123, 'foo bar' = 456"
    ),
}


@pytest.mark.parametrize(
    "sql",
    SQLGLOT_RELATED_NATIVE_NEGATIVE_CASES.values(),
    ids=SQLGLOT_RELATED_NATIVE_NEGATIVE_CASES.keys(),
)
def test_sqlglot_command_or_malformed_cases_rejected_by_native_trino(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.statement_count == 0
