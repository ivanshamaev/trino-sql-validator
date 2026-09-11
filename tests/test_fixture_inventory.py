"""Complete inventory of the SQL fixture contract."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import pytest
from trino_sql_validator import validate, validate_file

FIXTURES = Path(__file__).parent / "fixtures"


@dataclass(frozen=True)
class FixtureExpectation:
    valid: bool
    statement_count: int
    warning_names: tuple[str, ...] = ()
    error_fragment: str | None = None


FIXTURE_EXPECTATIONS = {
    "datamart_example.sql": FixtureExpectation(True, 1),
    "ddl_multi.sql": FixtureExpectation(True, 3),
    "empty.sql": FixtureExpectation(True, 0),
    "example-queries.sql": FixtureExpectation(True, 76),
    "iceberg_trino_sqldemo.sql": FixtureExpectation(True, 100),
    "invalid_one.sql": FixtureExpectation(False, 0, error_fragment="FORM"),
    "samples.sql": FixtureExpectation(True, 6),
    "sqlparser_merge_example.sql": FixtureExpectation(True, 1),
    "trino_dbt_customers.sql": FixtureExpectation(True, 1),
    "trino_iris_queries.sql": FixtureExpectation(True, 18),
    "trino_recursive_transformed.sql": FixtureExpectation(True, 4),
    "trino_reports_optimize.sql": FixtureExpectation(True, 17),
    "trino_reports_tests_schema.sql": FixtureExpectation(True, 6),
    "trino_specific.sql": FixtureExpectation(True, 1),
    "trino_tpch_queries.sql": FixtureExpectation(True, 39),
    "valid_multi.sql": FixtureExpectation(True, 3),
}

FIXTURE_FEATURES = {
    "datamart_example.sql": ("cte", "join", "window", "lambda", "map"),
    "ddl_multi.sql": ("create-table", "drop-table", "insert", "properties"),
    "empty.sql": ("empty", "comments"),
    "example-queries.sql": ("expressions", "json", "structural-types", "unnest"),
    "iceberg_trino_sqldemo.sql": ("iceberg", "ddl", "dml", "call", "time-travel"),
    "invalid_one.sql": ("negative", "multi-statement"),
    "samples.sql": ("geospatial", "ctas", "cte", "unnest"),
    "sqlparser_merge_example.sql": ("merge", "dml-branches"),
    "trino_dbt_customers.sql": ("legacy-jinja", "dbt", "cte"),
    "trino_iris_queries.sql": ("aggregate", "window", "filter", "map"),
    "trino_recursive_transformed.sql": ("recursive-cte", "subquery", "array-cast"),
    "trino_reports_optimize.sql": ("session", "alter-execute", "iceberg"),
    "trino_reports_tests_schema.sql": ("nested-row", "hive", "call", "view"),
    "trino_specific.sql": ("grouping", "having", "limit-all"),
    "trino_tpch_queries.sql": ("tpch", "join", "subquery", "ddl", "dml"),
    "valid_multi.sql": ("multi-statement", "select", "insert"),
}


def split_sql_statements(sql: str) -> list[str]:
    statements: list[str] = []
    start = 0
    position = 0
    quote: str | None = None
    dollar: str | None = None
    block_depth = 0
    line_comment = False
    routine = False
    routine_depth = 0
    after_end = False
    words: list[str] = []
    while position < len(sql):
        if line_comment:
            if sql[position] in "\r\n":
                line_comment = False
            position += 1
            continue
        if block_depth:
            if sql.startswith("/*", position):
                block_depth += 1
                position += 2
            elif sql.startswith("*/", position):
                block_depth -= 1
                position += 2
            else:
                position += 1
            continue
        if quote:
            if sql[position] == quote:
                if position + 1 < len(sql) and sql[position + 1] == quote:
                    position += 2
                    continue
                quote = None
            position += 1
            continue
        if dollar:
            if sql.startswith(dollar, position):
                position += len(dollar)
                dollar = None
            else:
                position += 1
            continue
        if sql.startswith("--", position):
            line_comment = True
            position += 2
            continue
        if sql.startswith("/*", position):
            block_depth = 1
            position += 2
            continue
        if sql[position] in "'\"`":
            quote = sql[position]
            position += 1
            continue
        if sql[position] == "$":
            end = position + 1
            while end < len(sql) and (sql[end].isalnum() or sql[end] == "_"):
                end += 1
            if end < len(sql) and sql[end] == "$":
                dollar = sql[position : end + 1]
                position = end + 1
                continue
        if sql[position].isalpha() or sql[position] == "_":
            end = position + 1
            while end < len(sql) and (sql[end].isalnum() or sql[end] == "_"):
                end += 1
            word = sql[position:end].lower()
            words.append(word)
            if len(words) <= 5 and word == "function" and "create" in words:
                routine = True
            if routine:
                if word == "end":
                    routine_depth = max(0, routine_depth - 1)
                    after_end = True
                elif word in {"begin", "case", "if", "loop", "repeat", "while"}:
                    if after_end:
                        after_end = False
                    elif word == "begin" or routine_depth:
                        routine_depth += 1
                elif after_end:
                    after_end = False
            position = end
            continue
        if sql[position] == ";" and routine_depth == 0:
            candidate = sql[start:position].strip()
            if candidate and validate(candidate).statement_count:
                statements.append(candidate)
            start = position + 1
            words = []
            routine = False
            after_end = False
        position += 1
    candidate = sql[start:].strip()
    if candidate and validate(candidate).statement_count:
        statements.append(candidate)
    return statements


def positive_fixture_statements() -> list[tuple[str, int, str]]:
    cases = []
    for filename, expected in FIXTURE_EXPECTATIONS.items():
        if not expected.valid:
            continue
        statements = split_sql_statements((FIXTURES / filename).read_text(encoding="utf-8"))
        cases.extend((filename, index, sql) for index, sql in enumerate(statements, 1))
    return cases


POSITIVE_FIXTURE_STATEMENTS = positive_fixture_statements()


def test_every_sql_fixture_has_an_explicit_expectation() -> None:
    actual = {path.name for path in FIXTURES.glob("*.sql")}
    assert actual == FIXTURE_EXPECTATIONS.keys()


def test_every_sql_fixture_has_an_explicit_feature_profile() -> None:
    assert FIXTURE_FEATURES.keys() == FIXTURE_EXPECTATIONS.keys()
    assert all(features for features in FIXTURE_FEATURES.values())


@pytest.mark.parametrize(
    ("filename", "expected"),
    FIXTURE_EXPECTATIONS.items(),
    ids=FIXTURE_EXPECTATIONS,
)
def test_sql_fixture_contract(filename: str, expected: FixtureExpectation) -> None:
    result = validate_file(FIXTURES / filename)

    assert result.valid is expected.valid
    assert result.statement_count == expected.statement_count
    assert tuple(warning.name for warning in result.warnings) == expected.warning_names

    if expected.error_fragment is None:
        assert result.error is None
    else:
        assert result.error is not None
        assert expected.error_fragment in result.error.message


def test_iceberg_fixture_has_no_catalog_warnings() -> None:
    result = validate_file(FIXTURES / "iceberg_trino_sqldemo.sql")

    assert result.warnings == ()


def test_positive_fixture_splitter_preserves_expected_statement_counts() -> None:
    actual: dict[str, int] = {}
    for filename, _, _ in POSITIVE_FIXTURE_STATEMENTS:
        actual[filename] = actual.get(filename, 0) + 1
    for filename, expected in FIXTURE_EXPECTATIONS.items():
        if expected.valid:
            assert actual.get(filename, 0) == expected.statement_count
    assert len(POSITIVE_FIXTURE_STATEMENTS) == 276


@pytest.mark.parametrize(
    ("filename", "statement_index", "sql"),
    POSITIVE_FIXTURE_STATEMENTS,
    ids=[f"{filename}::{index}" for filename, index, _ in POSITIVE_FIXTURE_STATEMENTS],
)
def test_every_positive_fixture_statement_independently(
    filename: str, statement_index: int, sql: str
) -> None:
    result = validate(sql)

    assert result.valid, f"{filename}::{statement_index}: {result.error}"
    assert result.statement_count == 1
    assert result.warnings == (), f"{filename}::{statement_index}: {result.warnings}"


def test_fixture_splitter_handles_comments_dollar_bodies_and_routine_semicolons() -> None:
    sql = """
        SELECT ';' /* ; */;
        CREATE FUNCTION f(x BIGINT) RETURNS BIGINT
        BEGIN
            RETURN x + 1;
        END;
        CREATE FUNCTION g() RETURNS VARCHAR LANGUAGE PYTHON AS $$return ';'$$;
        -- trailing ; comment
        SELECT 2
    """

    statements = split_sql_statements(sql)

    assert len(statements) == 4
    assert all(validate(statement).valid for statement in statements)
