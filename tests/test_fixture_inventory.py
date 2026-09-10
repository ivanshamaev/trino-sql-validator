"""Complete inventory of the SQL fixture contract."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import pytest
from trino_sql_validator import validate_file

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


def test_every_sql_fixture_has_an_explicit_expectation() -> None:
    actual = {path.name for path in FIXTURES.glob("*.sql")}
    assert actual == FIXTURE_EXPECTATIONS.keys()


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
