"""Complete inventory of the SQL fixture contract."""

from __future__ import annotations

import re
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
    independent_invalid: bool = False
    case_count: int | None = None


DATAMART_FEATURES = {
    "account_balance_anomaly_detection.sql": ("window", "anomaly-detection"),
    "ad_impressions_approx_metrics.sql": ("approximate-aggregates", "adtech"),
    "array_lambda_transform_filter_reduce.sql": ("array", "lambda", "reduce"),
    "attribution_first_last_touch.sql": ("window", "joins", "attribution"),
    "campaign_ctr_spend_metrics.sql": ("aggregate", "joins", "adtech"),
    "churn_reactivation_analysis.sql": ("window", "date-time", "churn"),
    "cohort_ltv_analysis.sql": ("create-view", "window", "cohort"),
    "conversion_funnel_stages.sql": ("window", "conditional", "funnel"),
    "corrupted_imports_safe_casting.sql": ("try-cast", "try", "data-quality"),
    "daily_sales_summary_partitioned.sql": ("ctas", "properties", "aggregate"),
    "deduplicate_transactions_qualify.sql": ("cte", "window", "deduplication"),
    "department_row_map_agg.sql": ("row", "map", "aggregate"),
    "events_timezone_conversion.sql": ("time-zone", "extract"),
    "http_payload_url_json_parse.sql": ("url", "json", "structural-types"),
    "inventory_levels_upsert.sql": ("merge", "dml-branches"),
    "invoice_overdue_payment_status.sql": ("window", "date-time", "conditional"),
    "ip_network_zone_classification.sql": ("ipaddress", "cast", "conditional"),
    "jdbc_catalog_metadata.sql": ("system-jdbc", "join", "ordering"),
    "keyword_performance_cpa.sql": ("aggregate", "having", "date"),
    "market_basket_product_pairs.sql": ("cte", "self-join", "subquery"),
    "org_hierarchy_recursive.sql": ("recursive-cte", "union"),
    "package_checkpoint_tracking.sql": ("unnest", "ordinality", "window"),
    "revenue_mom_yoy_growth.sql": ("window", "date-time", "growth"),
    "rfm_segmentation.sql": ("ctas", "window", "segmentation"),
    "sales_price_history_variance.sql": ("window", "range-join"),
    "top_products_category_ranking.sql": ("grouping-sets", "window", "ranking"),
    "transactions_currency_conversion.sql": ("correlated-subquery", "join"),
    "user_event_sequence_agg.sql": ("array", "ordered-aggregate", "sequence"),
    "user_sessionization.sql": ("window", "sessionization"),
    "vehicle_maintenance_downtime.sql": ("window", "date-time", "fleet"),
}


FIXTURE_EXPECTATIONS = {
    "datamart_example.sql": FixtureExpectation(True, 1),
    "ddl_multi.sql": FixtureExpectation(True, 3),
    "empty.sql": FixtureExpectation(True, 0),
    "example-queries.sql": FixtureExpectation(True, 76, warning_names=("all",)),
    "iceberg_trino_sqldemo.sql": FixtureExpectation(True, 100),
    "invalid_one.sql": FixtureExpectation(False, 0, error_fragment="FORM"),
    "samples.sql": FixtureExpectation(True, 6),
    "sqlparser_merge_example.sql": FixtureExpectation(True, 1),
    "trino_dbt_customers.sql": FixtureExpectation(True, 1),
    "trino_iris_queries.sql": FixtureExpectation(True, 18),
    "trino_iceberg_parser_test.sql": FixtureExpectation(True, 247),
    "trino_invalid_sql.sql": FixtureExpectation(
        False, 0, independent_invalid=True, case_count=236
    ),
    "trino_recursive_transformed.sql": FixtureExpectation(True, 4),
    "trino_reports_optimize.sql": FixtureExpectation(True, 17),
    "trino_reports_tests_schema.sql": FixtureExpectation(True, 6),
    "trino_specific.sql": FixtureExpectation(True, 1),
    "trino_tpch_queries.sql": FixtureExpectation(True, 39),
    "valid_multi.sql": FixtureExpectation(True, 3),
    **{
        f"datamarts/{filename}": FixtureExpectation(True, 1)
        for filename in DATAMART_FEATURES
    },
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
    "trino_iceberg_parser_test.sql": (
        "iceberg",
        "ddl",
        "dml",
        "sql-json",
        "security",
    ),
    "trino_invalid_sql.sql": ("negative", "independent-statements", "trino"),
    "trino_recursive_transformed.sql": ("recursive-cte", "subquery", "array-cast"),
    "trino_reports_optimize.sql": ("session", "alter-execute", "iceberg"),
    "trino_reports_tests_schema.sql": ("nested-row", "hive", "call", "view"),
    "trino_specific.sql": ("grouping", "having", "limit-all"),
    "trino_tpch_queries.sql": ("tpch", "join", "subquery", "ddl", "dml"),
    "valid_multi.sql": ("multi-statement", "select", "insert"),
    **{
        f"datamarts/{filename}": ("datamart", "native-trino-483", *features)
        for filename, features in DATAMART_FEATURES.items()
    },
}


def has_sql_content(sql: str) -> bool:
    position = 0
    block_depth = 0
    line_comment = False
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
        if sql.startswith("--", position):
            line_comment = True
            position += 2
            continue
        if sql.startswith("/*", position):
            block_depth = 1
            position += 2
            continue
        if not sql[position].isspace():
            return True
        position += 1
    return False


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
            if candidate and has_sql_content(candidate):
                statements.append(candidate)
            start = position + 1
            words = []
            routine = False
            after_end = False
        position += 1
    candidate = sql[start:].strip()
    if candidate and has_sql_content(candidate):
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
POSITIVE_STATEMENT_WARNING_NAMES = {
    ("example-queries.sql", 53): ("all",),
}


def independent_invalid_fixture_cases(filename: str) -> list[str]:
    sql = (FIXTURES / filename).read_text(encoding="utf-8")
    cases: list[str] = []
    for block in re.split(r"(?:\r?\n)[ \t]*(?:\r?\n)+", sql):
        lines = block.splitlines(keepends=True)
        while lines and lines[0].lstrip().startswith("--"):
            lines.pop(0)
        case = "".join(lines).strip("\r\n")
        if case.strip():
            cases.append(case)
    return cases


INDEPENDENT_INVALID_CASES = [
    (filename, index, sql)
    for filename, expected in FIXTURE_EXPECTATIONS.items()
    if expected.independent_invalid
    for index, sql in enumerate(independent_invalid_fixture_cases(filename), 1)
]

LOCATIONLESS_UPSTREAM_PARSER_CASES = {
    ("trino_invalid_sql.sql", 97),
    ("trino_invalid_sql.sql", 202),
}


def test_every_sql_fixture_has_an_explicit_expectation() -> None:
    actual = {path.relative_to(FIXTURES).as_posix() for path in FIXTURES.rglob("*.sql")}
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
    if expected.independent_invalid:
        cases = independent_invalid_fixture_cases(filename)
        assert len(cases) == expected.case_count
        return

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
    assert len(POSITIVE_FIXTURE_STATEMENTS) == 553


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
    expected_warnings = POSITIVE_STATEMENT_WARNING_NAMES.get((filename, statement_index), ())
    assert tuple(warning.name for warning in result.warnings) == expected_warnings


@pytest.mark.parametrize(
    ("filename", "case_index", "sql"),
    INDEPENDENT_INVALID_CASES,
    ids=[
        f"{filename}::{case_index:03d}"
        for filename, case_index, _ in INDEPENDENT_INVALID_CASES
    ],
)
def test_every_independent_invalid_fixture_case(
    filename: str, case_index: int, sql: str
) -> None:
    result = validate(sql, dialect="trino")

    assert not result.valid, f"{filename}::{case_index:03d}: unexpectedly valid\n{sql}"
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.message
    assert result.warnings == ()
    if (filename, case_index) not in LOCATIONLESS_UPSTREAM_PARSER_CASES:
        assert result.error.line is not None
        assert result.error.line > 0
        assert result.error.column is not None
        assert result.error.column > 0


def test_fixture_splitter_handles_comments_dollar_bodies_and_routine_semicolons() -> None:
    sql = """
        SELECT ';' /* ; */;
        CREATE FUNCTION f(x BIGINT) RETURNS BIGINT
        BEGIN
            RETURN x + 1;
        END;
        CREATE FUNCTION g() RETURNS VARCHAR LANGUAGE PYTHON AS $$
return ';'
$$;
        -- trailing ; comment
        SELECT 2
    """

    statements = split_sql_statements(sql)

    assert len(statements) == 4
    assert all(validate(statement).valid for statement in statements)


def test_fixture_splitter_does_not_hide_invalid_statements() -> None:
    statements = split_sql_statements("SELECT 1; SELECT * FORM t; -- comments only")

    assert len(statements) == 2
    assert validate(statements[0]).valid
    assert not validate(statements[1]).valid
