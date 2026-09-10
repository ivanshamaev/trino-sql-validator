"""Executable syntax matrix derived from the official Trino 483 documentation."""

from __future__ import annotations

import pytest
from trino_sql_validator import validate

SUPPORTED_SQL = {
    "match_recognize_basic": """
        SELECT *
        FROM orders MATCH_RECOGNIZE (
            PATTERN (A B+)
            DEFINE B AS totalprice < A.totalprice
        )
    """,
    "match_recognize_subset": """
        SELECT *
        FROM orders MATCH_RECOGNIZE (
            PARTITION BY custkey
            ORDER BY orderdate
            MEASURES LAST(U.totalprice) AS top_price
            ONE ROW PER MATCH
            AFTER MATCH SKIP PAST LAST ROW
            PATTERN (A B+ C+ D+)
            SUBSET U = (C, D)
            DEFINE
                B AS totalprice < PREV(totalprice),
                C AS totalprice > PREV(totalprice),
                D AS totalprice > PREV(totalprice)
        )
    """,
    "pivot_basic": """
        SELECT *
        FROM sales PIVOT (
            sum(amount)
            FOR month IN (1 AS jan, 2 AS feb)
        )
    """,
    "json_table_nested_path": """
        SELECT *
        FROM JSON_TABLE(
            '[{"id":1,"name":"Africa"}]',
            'strict $' COLUMNS (
                NESTED PATH 'strict $[*]' COLUMNS (
                    id INTEGER PATH 'strict $.id',
                    name VARCHAR PATH 'strict $.name'
                )
            )
        )
    """,
    "group_by_auto": "SELECT mktsegment, sum(acctbal) FROM shipping GROUP BY AUTO",
    "with_session": """
        WITH SESSION
            query_max_execution_time = '2h',
            example.query_partition_filter_required = true
        SELECT * FROM example.default.thetable LIMIT 100
    """,
    "fetch_with_ties": "SELECT * FROM nation ORDER BY name FETCH FIRST 5 ROWS WITH TIES",
    "limit_all": "SELECT * FROM nation LIMIT ALL",
    "tablesample": "SELECT * FROM nation TABLESAMPLE BERNOULLI (10)",
    "execute_immediate": "EXECUTE IMMEDIATE 'SELECT name FROM nation WHERE regionkey = ?' USING 1",
    "set_time_zone": "SET TIME ZONE 'Europe/Istanbul'",
    "show_create_function": "SHOW CREATE FUNCTION example.default.hello",
    "iceberg_numeric_version": "SELECT * FROM iceberg.test.orders FOR VERSION AS OF 12345",
    "iceberg_named_version": "SELECT * FROM iceberg.test.orders FOR VERSION AS OF 'audit-tag'",
    "iceberg_timestamp_as_of": """
        SELECT *
        FROM example.testdb.customer_orders
        FOR TIMESTAMP AS OF TIMESTAMP '2022-03-23 09:59:29.803 Europe/Vienna'
    """,
    "iceberg_date_as_of": """
        SELECT *
        FROM example.testdb.customer_orders
        FOR TIMESTAMP AS OF DATE '2022-03-23'
    """,
    "iceberg_variant_v3": """
        CREATE TABLE iceberg.default.v3 (id BIGINT, data VARIANT)
        WITH (format_version = 3)
    """,
    "iceberg_materialized_view_when_stale": """
        CREATE MATERIALIZED VIEW orders_summary
        GRACE PERIOD INTERVAL '1' HOUR
        WHEN STALE FAIL
        COMMENT 'Daily order summary'
        WITH (format = 'ORC')
        AS
            SELECT orderdate, sum(totalprice) AS price
            FROM orders
            GROUP BY orderdate
    """,
    "hive_bucketed_table": """
        CREATE TABLE example.web.page_views (
            view_time TIMESTAMP,
            user_id BIGINT,
            page_url VARCHAR,
            ds DATE,
            country VARCHAR
        )
        WITH (
            format = 'ORC',
            partitioned_by = ARRAY['ds', 'country'],
            bucketed_by = ARRAY['user_id'],
            bucket_count = 50
        )
    """,
    "hive_hdfs_external_location": """
        CREATE TABLE example.web.request_logs (request_time TIMESTAMP, url VARCHAR)
        WITH (
            format = 'TEXTFILE',
            external_location = 'hdfs://namenode:8020/data/logs/'
        )
    """,
    "hive_create_empty_partition": """
        CALL system.create_empty_partition(
            schema_name => 'web',
            table_name => 'page_views',
            partition_columns => ARRAY['ds', 'country'],
            partition_values => ARRAY['2016-08-09', 'US']
        )
    """,
    "hive_drop_stats_nested_array": """
        CALL system.drop_stats(
            schema_name => 'web',
            table_name => 'page_views',
            partition_values => ARRAY[ARRAY['2016-08-09', 'US']]
        )
    """,
    "deep_nested_structural_types": """
        CREATE TABLE nested_types (
            payload ROW(
                id BIGINT,
                events ARRAY(ROW(
                    ts TIMESTAMP(6) WITH TIME ZONE,
                    attrs MAP(VARCHAR, ROW(
                        score DECIMAL(12, 4),
                        flags ARRAY(VARCHAR)
                    ))
                ))
            ),
            lookup MAP(VARCHAR, ARRAY(ROW(
                k VARCHAR,
                v MAP(VARCHAR, BIGINT)
            )))
        )
    """,
}

DOCUMENTED_PARSER_GAPS = {
    "with_function": """
        WITH
            FUNCTION hello(name VARCHAR)
                RETURNS VARCHAR
                RETURN format('Hello %s!', name),
            FUNCTION bye(name VARCHAR)
                RETURNS VARCHAR
                RETURN format('Bye %s!', name)
        SELECT hello('Finn') || ' and ' || bye('Joe')
    """,
    "pivot_group_by": """
        SELECT *
        FROM sales PIVOT (
            sum(amount) AS total
            FOR month IN (1 AS jan, 2 AS feb, 3 AS mar)
            GROUP BY region
        )
    """,
    "corresponding": "SELECT 1 AS x UNION CORRESPONDING SELECT 2 AS x",
    "nearest": """
        SELECT trades.symbol, trades.ts, quotes.price
        FROM trades
        CROSS JOIN NEAREST (
            FROM quotes
            WHERE quotes.symbol = trades.symbol
            MATCH quotes.ts <= trades.ts
        )
    """,
}

DOCUMENTED_WARNING_REGRESSIONS = {
    "table_function_syntax": """
        SELECT *
        FROM TABLE(exclude_columns(
            input => TABLE(orders),
            columns => DESCRIPTOR(clerk, comment)
        ))
    """,
    "iceberg_table_changes": """
        SELECT *
        FROM TABLE(system.table_changes(
            schema_name => 'default',
            table_name => 't1',
            start_snapshot_id => 1,
            end_snapshot_id => 2
        ))
    """,
    "match_navigation_function": """
        SELECT *
        FROM orders MATCH_RECOGNIZE (
            PATTERN (A B+)
            DEFINE B AS totalprice < PREV(totalprice)
        )
    """,
}


@pytest.mark.parametrize(("feature", "sql"), SUPPORTED_SQL.items(), ids=SUPPORTED_SQL)
def test_documented_trino_483_supported_syntax(feature: str, sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, f"{feature}: {result.error}"
    assert result.statement_count == 1
    assert result.warnings == (), f"{feature}: {result.warnings}"


@pytest.mark.xfail(strict=True, reason="documented Trino 483 parser gap planned for v0.11.0")
@pytest.mark.parametrize(
    ("feature", "sql"), DOCUMENTED_PARSER_GAPS.items(), ids=DOCUMENTED_PARSER_GAPS
)
def test_documented_trino_483_parser_gap(feature: str, sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, f"{feature}: {result.error}"


@pytest.mark.parametrize(
    ("feature", "sql"),
    DOCUMENTED_WARNING_REGRESSIONS.items(),
    ids=DOCUMENTED_WARNING_REGRESSIONS,
)
def test_documented_trino_483_warning_regression(feature: str, sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, f"{feature}: {result.error}"
    assert result.warnings == (), f"{feature}: {result.warnings}"
