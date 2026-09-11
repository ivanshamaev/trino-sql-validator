from __future__ import annotations

import pytest
from trino_sql_validator import FunctionWarning, TypeWarning, validate


@pytest.mark.parametrize(
    ("sql", "kind", "name"),
    [
        ("SELECT CAST(NULL AS ROW(x bignum))", TypeWarning, "bignum"),
        ("SELECT CAST(NULL AS ARRAY(ROW(x bignum)))", TypeWarning, "bignum"),
        ("SELECT json_array(1 RETURNING bignum)", TypeWarning, "bignum"),
        ("SELECT json_value('{}', '$.x' RETURNING bignum)", TypeWarning, "bignum"),
        (
            "SELECT * FROM JSON_TABLE('{}', '$' COLUMNS(x bignum PATH '$.x'))",
            TypeWarning,
            "bignum",
        ),
        ("SELECT bignum 'x'", TypeWarning, "bignum"),
        ("DROP FUNCTION f(bignum)", TypeWarning, "bignum"),
        ("PREPARE p FROM CREATE TABLE t(x bignum)", TypeWarning, "bignum"),
        ("ALTER TABLE t ADD COLUMN payload.x bignum", TypeWarning, "bignum"),
        (
            "ALTER TABLE t ALTER COLUMN payload.x SET DATA TYPE bignum",
            TypeWarning,
            "bignum",
        ),
        (
            "ALTER TABLE t ADD COLUMN x bignum WITH (value = missing_fn(1))",
            TypeWarning,
            "bignum",
        ),
        (
            "ALTER TABLE t ADD COLUMN x bignum WITH (value = missing_fn(1))",
            FunctionWarning,
            "missing_fn",
        ),
        (
            "EXPLAIN ALTER TABLE t SET PROPERTIES value = missing_fn(1)",
            FunctionWarning,
            "missing_fn",
        ),
        ("SELECT CAST(1 AS INT64)", TypeWarning, "int64"),
        ("CREATE TABLE t(x STRING)", TypeWarning, "string"),
        ("CREATE TABLE t(x BYTEA)", TypeWarning, "bytea"),
    ],
)
def test_metadata_is_preserved_in_every_supported_context(
    sql: str, kind: type[FunctionWarning | TypeWarning], name: str
) -> None:
    result = validate(sql)

    assert result.valid, result.error
    matching = [warning for warning in result.warnings if isinstance(warning, kind) and warning.name == name]
    assert len(matching) == 1
    assert matching[0].line == 1
    assert matching[0].column == sql.lower().index(name) + 1


def test_synthetic_metadata_does_not_duplicate_one_source_type() -> None:
    sql = "CREATE FUNCTION f() RETURNS BIGINT BEGIN DECLARE x, y bignum; RETURN 1; END"

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_types == ["bignum"]


@pytest.mark.parametrize(
    "sql",
    [
        "EXPLAIN WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x SELECT f(1)",
        "PREPARE p FROM WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x SELECT f(1)",
        "CREATE TABLE t AS WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x SELECT f(1)",
        "CREATE VIEW v AS WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x SELECT f(1)",
        "INSERT INTO t WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x SELECT f(1)",
        "EXPLAIN WITH SESSION query_max_memory = 1 SELECT 1",
        "PREPARE p FROM WITH SESSION query_max_memory = 1 SELECT 1",
        (
            "WITH SESSION query_max_memory = 1 "
            "WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x "
            "WITH q AS (SELECT f(1) AS x) SELECT x FROM q"
        ),
    ],
)
def test_query_prefixes_compose_with_statement_wrappers(sql: str) -> None:
    result = validate(sql)

    assert result.valid, result.error
    assert result.warnings == ()


def test_inline_function_scope_is_limited_to_its_statement() -> None:
    sql = (
        "WITH FUNCTION local_probe(x BIGINT) RETURNS BIGINT RETURN x "
        "SELECT local_probe(1); SELECT local_probe(2)"
    )

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_functions == ["local_probe"]
    warning = result.warnings[0]
    assert warning.column == sql.rindex("local_probe") + 1


def test_qualified_name_is_not_exempted_as_an_inline_function() -> None:
    sql = (
        "WITH FUNCTION local_probe(x BIGINT) RETURNS BIGINT RETURN x "
        "SELECT remote.ns.local_probe(1)"
    )

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_functions == ["local_probe"]
    assert result.warnings[0].column == sql.rindex("local_probe") + 1


def test_inline_functions_can_reference_each_other_in_the_same_scope() -> None:
    sql = (
        "WITH FUNCTION first_f(x BIGINT) RETURNS BIGINT RETURN second_f(x), "
        "FUNCTION second_f(x BIGINT) RETURNS BIGINT RETURN x "
        "SELECT first_f(1)"
    )

    result = validate(sql)

    assert result.valid, result.error
    assert result.warnings == ()
