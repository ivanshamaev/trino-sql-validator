"""Curated Trino 483 syntax matrix.

Cases are adapted from Apache Trino parser tests and documentation, licensed
under Apache-2.0. Sources:
https://github.com/trinodb/trino/tree/483/core/trino-parser/src/test/java/io/trino/sql/parser
https://trino.io/docs/483/
"""

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
    "group_by_quantifier": "SELECT a, b, sum(c) FROM t GROUP BY DISTINCT ROLLUP ((a, b), c)",
    "empty_grouping_elements": "SELECT 1 GROUP BY ROLLUP (), CUBE ()",
    "at_local": "SELECT timestamp '2024-01-01 12:00:00' AT LOCAL",
    "scalar_values_relation": "SELECT * FROM LATERAL (VALUES 1, 2)",
    "corresponding": "SELECT 1 AS x UNION CORRESPONDING BY (x) SELECT 2 AS x",
    "pivot_group_by": """
        SELECT *
        FROM sales PIVOT (
            sum(amount) AS total
            FOR month IN (1 AS jan, 2 AS feb, 3 AS mar)
            GROUP BY region
        )
    """,
    "nearest": """
        SELECT *
        FROM trades
        CROSS JOIN NEAREST (
            FROM quotes
            WHERE quotes.symbol = trades.symbol
            MATCH quotes.ts <= trades.ts
        )
    """,
    "with_session": """
        WITH SESSION
            query_max_execution_time = '2h',
            example.query_partition_filter_required = true
        SELECT * FROM example.default.thetable LIMIT 100
    """,
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
    "row_expansion": "SELECT ROW(1, 'a', true).* AS (f1, f2, f3)",
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

# One committed case per mismatch family found by the differential v0.10.0
# baseline. The stable TSV-* identifiers are work-item IDs used by the plan and
# by test output. The pinned direct-string subset currently has no unsupported
# syntax gaps: all 456 extracted statement cases and all 68 types are green.
APACHE_TRINO_483_REGRESSION_MATRIX = {
    "TSV-P1-ICEBERG-BRANCH": "INSERT INTO orders @ dev VALUES 1",
    "TSV-P1-ASCII-IDENTIFIERS": 'SELECT "имя" FROM orders',
    "TSV-P2-CATALOG-DDL": "CREATE CATALOG IF NOT EXISTS hive USING hive WITH (connector = 'hive')",
    "TSV-P2-BRANCH-DDL": "CREATE OR REPLACE BRANCH audit IN TABLE orders FROM main",
    "TSV-P2-ALTER-NESTED-COLUMN": "ALTER TABLE orders RENAME COLUMN payload.old TO new",
    "TSV-P2-ALTER-OWNED-ENTITY": "ALTER QUARK hive.default.orders SET AUTHORIZATION ROLE analyst",
    "TSV-P2-ROLE-CATALOG": "CREATE ROLE analyst WITH ADMIN CURRENT_USER IN hive",
    "TSV-P2-GRANT-ROLE": "GRANT analyst TO USER alice GRANTED BY CURRENT_ROLE IN hive",
    "TSV-P2-GRANT-PRIVILEGE": "GRANT CREATE BRANCH ON TABLE orders TO ROLE analyst",
    "TSV-P2-DENY-PRIVILEGE": "DENY DELETE ON TABLE orders TO USER alice",
    "TSV-P2-SHOW-LIKE": "SHOW FUNCTIONS FROM hive.default LIKE '%$_%' ESCAPE '$'",
    "TSV-P2-DESCRIBE-QUERY": "DESCRIBE OUTPUT (SELECT marh(1))",
    "TSV-P2-ANALYZE-PROPERTIES": "ANALYZE orders WITH (columns = ARRAY['id'])",
    "TSV-P2-CTAS-ALIASES": "CREATE TABLE copy(id) AS SELECT id FROM orders WITH NO DATA",
    "TSV-P2-TABLE-LIKE": "CREATE TABLE copy (LIKE orders INCLUDING PROPERTIES)",
    "TSV-P2-COLUMN-PROPERTIES": "CREATE TABLE t (value VARCHAR WITH (compression = 'LZ4'))",
    "TSV-P2-VIEW-OPTIONS": "CREATE VIEW v COMMENT 'v' SECURITY DEFINER AS SELECT 1",
    "TSV-P2-NONRESERVED": "SELECT ALL, SOME, ANY FROM orders",
    "TSV-P2-JSON-TABLE-SCALAR": (
        "SELECT * FROM JSON_TABLE(payload, '$' COLUMNS("
        "value VARCHAR FORMAT JSON ENCODING UTF16 PATH '$.value' "
        "WITH WRAPPER KEEP QUOTES EMPTY ARRAY ON EMPTY) EMPTY ON ERROR)"
    ),
    "TSV-P3-WITH-SESSION": "WITH SESSION query_max_execution_time = '2h' SELECT 1",
    "TSV-P3-CORRESPONDING": "SELECT 1 AS x UNION CORRESPONDING BY (x) SELECT 2 AS x",
    "TSV-P3-PIVOT-GROUP": "SELECT * FROM t PIVOT (sum(v) FOR k IN (1) GROUP BY g)",
    "TSV-P3-NEAREST": "SELECT * FROM a CROSS JOIN NEAREST (FROM b MATCH b.ts <= a.ts)",
    "TSV-P3-ROW-EXPANSION": "SELECT ROW(1, 2).* AS (x, y)",
    "TSV-P3-TABLE-FUNCTION": (
        "SELECT * FROM TABLE(f(input => TABLE(orders) AS o "
        "PARTITION BY (id) KEEP WHEN EMPTY ORDER BY (id)))"
    ),
    "TSV-P4-EXPRESSION": "SELECT bigint::parse(value => '42') BETWEEN SYMMETRIC 1 AND 100",
    "TSV-P4-STRUCTURAL-TYPE": "SELECT CAST(NULL AS MAP<BIGINT, VARCHAR> ARRAY)",
    "TSV-P4-ROUTINE": "CREATE FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x + 1",
}


@pytest.mark.parametrize(("feature", "sql"), SUPPORTED_SQL.items(), ids=SUPPORTED_SQL)
def test_documented_trino_483_supported_syntax(feature: str, sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, f"{feature}: {result.error}"
    assert result.statement_count == 1
    assert result.warnings == (), f"{feature}: {result.warnings}"


@pytest.mark.parametrize(
    ("feature", "sql"),
    DOCUMENTED_WARNING_REGRESSIONS.items(),
    ids=DOCUMENTED_WARNING_REGRESSIONS,
)
def test_documented_trino_483_warning_regression(feature: str, sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, f"{feature}: {result.error}"
    assert result.warnings == (), f"{feature}: {result.warnings}"


@pytest.mark.parametrize(
    ("work_item", "sql"),
    APACHE_TRINO_483_REGRESSION_MATRIX.items(),
    ids=APACHE_TRINO_483_REGRESSION_MATRIX,
)
def test_apache_trino_483_regression_matrix(work_item: str, sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, f"{work_item}: {result.error}"
    assert result.statement_count == 1
