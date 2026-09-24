-- =====================================================================
-- Trino + Iceberg: набор statements для тестирования SQL-парсера
-- Каталог: iceberg, схема: parser_test. tpch.tiny используется как источник данных.
-- Каждый statement завершается ';' в конце строки, ';' внутри литералов нет.
-- Некоторые statements зависят от окружения (права, реальные snapshot id, S3-пути),
-- но синтаксически валидны для актуальных версий Trino.
-- =====================================================================

-- ---------------------------------------------------------------------
-- 1. SCHEMA
-- ---------------------------------------------------------------------
CREATE SCHEMA IF NOT EXISTS iceberg.parser_test;
CREATE SCHEMA iceberg.parser_test_tmp WITH (location = 's3://warehouse/parser_test_tmp/');
ALTER SCHEMA iceberg.parser_test_tmp SET AUTHORIZATION USER admin;
SHOW CREATE SCHEMA iceberg.parser_test_tmp;
DROP SCHEMA IF EXISTS iceberg.parser_test_tmp;
USE iceberg.parser_test;
SHOW SCHEMAS FROM iceberg LIKE 'parser%';

-- ---------------------------------------------------------------------
-- 2. CREATE TABLE (разные свойства, типы, партиционирование)
-- ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS customers (
    custkey bigint NOT NULL COMMENT 'customer id',
    name varchar,
    address varchar,
    nationkey bigint,
    phone varchar,
    acctbal double,
    mktsegment varchar,
    comment varchar
)
COMMENT 'Customers dimension'
WITH (
    format = 'PARQUET',
    format_version = 2,
    location = 's3://warehouse/parser_test/customers',
    extra_properties = MAP(ARRAY['write.target-file-size-bytes'], ARRAY['134217728'])
);

CREATE TABLE orders (
    orderkey bigint,
    custkey bigint,
    orderstatus varchar,
    totalprice double,
    orderdate date,
    orderpriority varchar,
    clerk varchar,
    shippriority integer,
    comment varchar
)
WITH (
    format = 'ORC',
    partitioning = ARRAY['month(orderdate)', 'bucket(custkey, 16)'],
    sorted_by = ARRAY['orderkey'],
    orc_bloom_filter_columns = ARRAY['custkey'],
    orc_bloom_filter_fpp = 0.05
);

CREATE TABLE lineitem (
    orderkey bigint,
    partkey bigint,
    suppkey bigint,
    linenumber integer,
    quantity double,
    extendedprice double,
    discount double,
    tax double,
    returnflag varchar,
    linestatus varchar,
    shipdate date,
    commitdate date,
    receiptdate date,
    shipinstruct varchar,
    shipmode varchar,
    comment varchar
)
WITH (
    format = 'PARQUET',
    partitioning = ARRAY['year(shipdate)', 'truncate(shipmode, 2)', 'linestatus']
);

CREATE TABLE events (
    event_id uuid,
    user_id bigint,
    event_ts timestamp(6) with time zone,
    event_type varchar,
    payload varchar,
    props map(varchar, varchar),
    tags array(varchar),
    geo row(lat double, lon double)
)
WITH (
    format = 'PARQUET',
    partitioning = ARRAY['hour(event_ts)', 'bucket(user_id, 32)'],
    sorted_by = ARRAY['event_ts DESC']
);

CREATE TABLE all_types (
    c_boolean boolean,
    c_integer integer,
    c_bigint bigint,
    c_real real,
    c_double double,
    c_decimal decimal(38, 10),
    c_varchar varchar,
    c_varbinary varbinary,
    c_date date,
    c_time time(6),
    c_timestamp timestamp(6),
    c_timestamptz timestamp(6) with time zone,
    c_uuid uuid,
    c_array array(integer),
    c_map map(varchar, double),
    c_row row(a integer, b row(c varchar, d array(bigint)))
);

-- CTAS
CREATE TABLE nation WITH (format = 'PARQUET') AS SELECT * FROM tpch.tiny.nation;
CREATE TABLE region (regionkey, region_name, region_comment) AS SELECT regionkey, name, comment FROM tpch.tiny.region;
CREATE TABLE orders_1996
WITH (format = 'PARQUET', partitioning = ARRAY['day(orderdate)'])
AS SELECT * FROM tpch.tiny.orders WHERE orderdate >= DATE '1996-01-01' AND orderdate < DATE '1997-01-01';
CREATE TABLE orders_empty AS SELECT * FROM orders WITH NO DATA;
CREATE TABLE IF NOT EXISTS orders_like (LIKE orders INCLUDING PROPERTIES);
CREATE TABLE orders_cte AS
WITH big AS (SELECT * FROM orders WHERE totalprice > 100000)
SELECT custkey, count(*) AS cnt FROM big GROUP BY custkey;
CREATE OR REPLACE TABLE orders_cte AS SELECT custkey, sum(totalprice) AS total FROM orders GROUP BY custkey;
CREATE TABLE orders_stage AS SELECT orderkey, totalprice, CAST('U' AS varchar) AS op FROM tpch.tiny.orders WHERE orderkey < 100;

-- ---------------------------------------------------------------------
-- 3. INSERT
-- ---------------------------------------------------------------------
INSERT INTO customers SELECT custkey, name, address, nationkey, phone, acctbal, mktsegment, comment FROM tpch.tiny.customer;
INSERT INTO orders SELECT orderkey, custkey, orderstatus, totalprice, orderdate, orderpriority, clerk, shippriority, comment FROM tpch.tiny.orders;
INSERT INTO lineitem SELECT * FROM tpch.tiny.lineitem;
INSERT INTO orders (orderkey, custkey, orderstatus, totalprice, orderdate)
VALUES (900000001, 1, 'O', 123.45, DATE '2024-01-15'), (900000002, 2, 'F', 67.89, DATE '2024-02-20');
INSERT INTO orders (orderkey, custkey, totalprice, orderdate)
WITH src AS (SELECT 900000003 AS k, BIGINT '3' AS c, 10.5E0 AS p, DATE '2024-03-01' AS d)
SELECT k, c, p, d FROM src;
INSERT INTO events
VALUES (
    uuid(), 1, TIMESTAMP '2024-05-01 10:00:00.123456 UTC', 'click', '{"page":"home","n":1}',
    MAP(ARRAY['browser', 'os'], ARRAY['firefox', 'linux']), ARRAY['web', 'mobile'],
    CAST(ROW(60.17, 24.94) AS ROW(lat double, lon double))
);
INSERT INTO all_types VALUES (
    true, 1, 9223372036854775807, REAL '1.5', DOUBLE '2.5', DECIMAL '12345.6789012345', 'text', X'DEADBEEF',
    DATE '2024-02-29', TIME '12:34:56.123456', TIMESTAMP '2024-02-29 12:34:56.123456',
    TIMESTAMP '2024-02-29 12:34:56.123456 Europe/Helsinki', UUID '12151fd2-7586-11e9-8f9e-2a86e4085a59',
    ARRAY[1, 2, 3], MAP(ARRAY['a', 'b'], ARRAY[1.0E0, 2.0E0]),
    CAST(ROW(1, ROW('x', ARRAY[BIGINT '1'])) AS ROW(a integer, b ROW(c varchar, d ARRAY(bigint))))
);

-- ---------------------------------------------------------------------
-- 4. COMMENT / ALTER TABLE / ANALYZE / table procedures
-- ---------------------------------------------------------------------
COMMENT ON TABLE orders IS 'Orders fact table';
COMMENT ON COLUMN orders.clerk IS 'Clerk name';
COMMENT ON COLUMN orders.clerk IS NULL;
ALTER TABLE orders ADD COLUMN IF NOT EXISTS discount double COMMENT 'discount pct';
ALTER TABLE orders ADD COLUMN nested row(a integer, b varchar);
ALTER TABLE orders RENAME COLUMN discount TO discount_pct;
ALTER TABLE orders ALTER COLUMN shippriority SET DATA TYPE bigint;
ALTER TABLE orders DROP COLUMN IF EXISTS nested;
ALTER TABLE customers ALTER COLUMN custkey DROP NOT NULL;
ALTER TABLE events ADD COLUMN geo.altitude double;
ALTER TABLE events DROP COLUMN geo.altitude;
ALTER TABLE orders SET PROPERTIES format_version = 2;
ALTER TABLE orders SET PROPERTIES partitioning = ARRAY['month(orderdate)'];
ALTER TABLE orders SET PROPERTIES extra_properties = MAP(ARRAY['write.metadata.delete-after-commit.enabled'], ARRAY['true']);
ALTER TABLE orders RENAME TO orders_v2;
ALTER TABLE IF EXISTS orders_v2 RENAME TO orders;
ALTER TABLE orders EXECUTE optimize;
ALTER TABLE orders EXECUTE optimize(file_size_threshold => '128MB') WHERE orderdate >= DATE '1996-01-01';
ALTER TABLE orders EXECUTE optimize_manifests;
ALTER TABLE orders EXECUTE expire_snapshots(retention_threshold => '7d');
ALTER TABLE orders EXECUTE remove_orphan_files(retention_threshold => '7d');
ALTER TABLE orders EXECUTE drop_extended_stats;
ALTER TABLE orders_empty EXECUTE add_files(location => 's3://warehouse/external/orders_files', format => 'PARQUET');
ALTER TABLE orders_empty EXECUTE add_files_from_table(schema_name => 'parser_test', table_name => 'orders_1996');
ANALYZE orders;
ANALYZE orders WITH (columns = ARRAY['orderkey', 'custkey']);

-- ---------------------------------------------------------------------
-- 5. UPDATE / DELETE / MERGE / TRUNCATE
-- ---------------------------------------------------------------------
UPDATE orders SET totalprice = totalprice * 1.1, orderpriority = '1-URGENT' WHERE orderstatus = 'O' AND orderdate < DATE '1995-01-01';
UPDATE orders SET clerk = 'Clerk#000000001' WHERE custkey IN (SELECT custkey FROM customers WHERE mktsegment = 'AUTOMOBILE');
UPDATE customers SET acctbal = acctbal + 100 WHERE nationkey = 1;
DELETE FROM orders WHERE orderdate < DATE '1992-02-01';
DELETE FROM orders WHERE custkey IN (SELECT custkey FROM customers WHERE acctbal < 0);
DELETE FROM lineitem WHERE shipdate BETWEEN DATE '1998-01-01' AND DATE '1998-06-30' AND shipmode = 'MAIL';

MERGE INTO orders AS t
USING orders_stage AS s
ON t.orderkey = s.orderkey
WHEN MATCHED AND s.op = 'D' THEN DELETE
WHEN MATCHED THEN UPDATE SET totalprice = s.totalprice, comment = 'merged'
WHEN NOT MATCHED THEN INSERT (orderkey, totalprice, orderdate) VALUES (s.orderkey, s.totalprice, current_date);

MERGE INTO customers c
USING (VALUES (1, 'New Name'), (999999, 'Brand New')) AS s (custkey, name)
ON c.custkey = s.custkey
WHEN MATCHED THEN UPDATE SET name = s.name
WHEN NOT MATCHED AND s.custkey > 0 THEN INSERT VALUES (s.custkey, s.name, NULL, NULL, NULL, NULL, NULL, NULL);

TRUNCATE TABLE orders_empty;

-- ---------------------------------------------------------------------
-- 6. Views & Materialized views
-- ---------------------------------------------------------------------
CREATE VIEW v_open_orders AS SELECT orderkey, custkey, totalprice FROM orders WHERE orderstatus = 'O';
CREATE OR REPLACE VIEW v_open_orders COMMENT 'open orders' SECURITY INVOKER AS SELECT orderkey, custkey, totalprice, orderdate FROM orders WHERE orderstatus = 'O';
CREATE VIEW v_cust_totals SECURITY DEFINER AS
WITH t AS (SELECT custkey, sum(totalprice) AS total FROM orders GROUP BY custkey)
SELECT c.name, t.total FROM customers c JOIN t ON c.custkey = t.custkey;
COMMENT ON VIEW v_open_orders IS 'Open orders view';
COMMENT ON COLUMN v_open_orders.orderkey IS 'PK';
ALTER VIEW v_open_orders RENAME TO v_open_orders2;
ALTER VIEW v_open_orders2 SET AUTHORIZATION USER admin;
SHOW CREATE VIEW v_cust_totals;
CREATE MATERIALIZED VIEW mv_orders_daily
GRACE PERIOD INTERVAL '1' HOUR
COMMENT 'daily order totals'
WITH (format = 'PARQUET', partitioning = ARRAY['month(orderdate)'])
AS SELECT orderdate, count(*) AS cnt, sum(totalprice) AS total FROM orders GROUP BY orderdate;
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_customers_seg AS SELECT mktsegment, count(*) AS cnt FROM customers GROUP BY mktsegment;
REFRESH MATERIALIZED VIEW mv_orders_daily;
ALTER MATERIALIZED VIEW mv_orders_daily RENAME TO mv_orders_daily2;
ALTER MATERIALIZED VIEW mv_orders_daily2 SET PROPERTIES format = 'ORC';
SHOW CREATE MATERIALIZED VIEW mv_orders_daily2;
DROP MATERIALIZED VIEW IF EXISTS mv_customers_seg;

-- ---------------------------------------------------------------------
-- 7. Time travel, метаданные Iceberg, процедуры, table functions
-- ---------------------------------------------------------------------
SELECT * FROM orders FOR VERSION AS OF 8954597067493422955;
SELECT * FROM orders FOR TIMESTAMP AS OF TIMESTAMP '2024-01-01 00:00:00 UTC';
SELECT * FROM orders FOR TIMESTAMP AS OF CAST('2024-01-01 00:00:00 Europe/Helsinki' AS timestamp(3) with time zone) WHERE totalprice > 100;
SELECT * FROM orders FOR VERSION AS OF 'audit_branch' LIMIT 10;
SELECT * FROM "orders$snapshots" ORDER BY committed_at DESC;
SELECT * FROM "orders$history";
SELECT * FROM "orders$files";
SELECT * FROM "orders$manifests";
SELECT * FROM "orders$partitions";
SELECT * FROM "orders$refs";
SELECT * FROM "orders$properties";
SELECT * FROM "orders$metadata_log_entries";
SELECT s.snapshot_id, s.operation, h.is_current_ancestor FROM "orders$snapshots" s JOIN "orders$history" h ON s.snapshot_id = h.snapshot_id;
SELECT * FROM TABLE(iceberg.system.table_changes(schema_name => 'parser_test', table_name => 'orders', start_snapshot_id => 1, end_snapshot_id => 2));
SELECT * FROM TABLE(exclude_columns(input => TABLE(orders), columns => DESCRIPTOR(comment, clerk))) LIMIT 5;
SELECT * FROM TABLE(sequence(start => 1, stop => 10, step => 3));
CALL iceberg.system.rollback_to_snapshot('parser_test', 'orders', 8954597067493422955);
CALL iceberg.system.register_table(schema_name => 'parser_test', table_name => 'orders_reg', table_location => 's3://warehouse/parser_test/orders_reg');
CALL iceberg.system.unregister_table(schema_name => 'parser_test', table_name => 'orders_reg');
CALL iceberg.system.migrate('parser_test', 'hive_legacy_table');

-- ---------------------------------------------------------------------
-- 8. SELECT: базовые конструкции, joins
-- ---------------------------------------------------------------------
SELECT 1;
SELECT orderkey, custkey, totalprice AS price, orderdate FROM orders WHERE orderstatus = 'F' AND totalprice BETWEEN 1000 AND 50000 ORDER BY totalprice DESC LIMIT 10;
SELECT DISTINCT orderstatus, orderpriority FROM orders ORDER BY 1, 2;
SELECT o.*, c.name FROM orders AS o JOIN customers c ON o.custkey = c.custkey WHERE c.mktsegment IN ('BUILDING', 'MACHINERY') AND o.orderdate >= DATE '1996-01-01';
SELECT "$path", "$file_modified_time", orderkey FROM orders LIMIT 5;
SELECT current_date, current_timestamp, localtimestamp, localtime, current_time, current_user, current_catalog, current_schema, current_path;
TABLE nation ORDER BY nationkey LIMIT 3;
VALUES (1, 'a'), (2, 'b');
SELECT * FROM (VALUES (1, 'a'), (2, 'b')) AS t (id, val);
SELECT o.orderkey, c.name FROM orders o LEFT JOIN customers c USING (custkey);
SELECT o.orderkey, c.name FROM orders o RIGHT JOIN customers c ON o.custkey = c.custkey;
SELECT o.orderkey, c.name FROM orders o FULL OUTER JOIN customers c ON o.custkey = c.custkey;
SELECT n.name, r.region_name FROM nation n CROSS JOIN region r;
SELECT n.name, r.region_name FROM nation n, region r WHERE n.regionkey = r.regionkey;
SELECT c.custkey, o.orderkey
FROM customers c
CROSS JOIN LATERAL (SELECT orderkey FROM orders WHERE orders.custkey = c.custkey ORDER BY totalprice DESC LIMIT 3) o;
SELECT a.orderkey, b.orderkey FROM orders a JOIN orders b ON a.custkey = b.custkey AND a.orderkey < b.orderkey AND b.totalprice > a.totalprice * 1.5;
SELECT * FROM orders TABLESAMPLE BERNOULLI (10);
SELECT * FROM orders TABLESAMPLE SYSTEM (5) WHERE orderstatus = 'O';

-- ---------------------------------------------------------------------
-- 9. Агрегации
-- ---------------------------------------------------------------------
SELECT custkey, count(*) AS cnt, sum(totalprice) AS total FROM orders GROUP BY custkey HAVING count(*) > 5 ORDER BY total DESC;
SELECT count(*) AS all_rows, count(DISTINCT custkey) AS uniq, count_if(totalprice > 1000) AS big, sum(totalprice) FILTER (WHERE orderstatus = 'O') AS open_total, avg(totalprice) AS avg_price FROM orders;
SELECT orderstatus, orderpriority, count(*) FROM orders GROUP BY ROLLUP (orderstatus, orderpriority);
SELECT orderstatus, orderpriority, count(*) FROM orders GROUP BY CUBE (orderstatus, orderpriority);
SELECT orderstatus, orderpriority, count(*), grouping(orderstatus, orderpriority) AS g FROM orders GROUP BY GROUPING SETS ((orderstatus), (orderpriority), ());
SELECT custkey, array_agg(orderkey ORDER BY totalprice DESC) AS keys, listagg(orderstatus, ',') WITHIN GROUP (ORDER BY orderkey) AS statuses FROM orders GROUP BY custkey;
SELECT approx_distinct(custkey), approx_percentile(totalprice, 0.95), min_by(orderkey, totalprice), max_by(orderkey, totalprice, 3), arbitrary(orderstatus), bool_or(totalprice > 0), every(totalprice > 0) FROM orders;
SELECT map_agg(orderkey, totalprice), histogram(orderstatus), multimap_agg(custkey, orderkey) FROM orders WHERE orderkey < 100;
SELECT date_trunc('month', orderdate) AS m, year(orderdate) AS y, sum(totalprice) FROM orders GROUP BY 1, 2 ORDER BY 1;
SELECT shipmode, returnflag, sum(quantity), sum(extendedprice * (1 - discount)) FROM lineitem GROUP BY shipmode, returnflag ORDER BY shipmode NULLS LAST, returnflag DESC NULLS FIRST;

-- ---------------------------------------------------------------------
-- 10. Оконные функции
-- ---------------------------------------------------------------------
SELECT orderkey, custkey, row_number() OVER (PARTITION BY custkey ORDER BY orderdate) AS rn, rank() OVER (ORDER BY totalprice DESC) AS rnk, dense_rank() OVER (ORDER BY totalprice) AS drnk, ntile(4) OVER (ORDER BY totalprice) AS q FROM orders;
SELECT orderkey, lag(totalprice, 1, 0) OVER (PARTITION BY custkey ORDER BY orderdate) AS prev, lead(totalprice) OVER (PARTITION BY custkey ORDER BY orderdate) AS next, first_value(totalprice) OVER (PARTITION BY custkey ORDER BY orderdate) AS fv, last_value(totalprice) OVER (PARTITION BY custkey ORDER BY orderdate ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS lv FROM orders;
SELECT orderdate, sum(totalprice) OVER (ORDER BY orderdate ROWS BETWEEN 2 PRECEDING AND CURRENT ROW) AS moving_sum FROM orders;
SELECT orderdate, avg(totalprice) OVER (ORDER BY orderdate RANGE BETWEEN INTERVAL '7' DAY PRECEDING AND CURRENT ROW) AS avg7 FROM orders;
SELECT orderdate, count(*) OVER (ORDER BY orderdate GROUPS BETWEEN 1 PRECEDING AND 1 FOLLOWING) AS cnt FROM orders;
SELECT orderkey, nth_value(totalprice, 2) IGNORE NULLS OVER (PARTITION BY custkey ORDER BY orderdate) AS second_price, percent_rank() OVER (ORDER BY totalprice) AS pr, cume_dist() OVER (ORDER BY totalprice) AS cd FROM orders;

-- ---------------------------------------------------------------------
-- 11. Подзапросы, CTE, set operations
-- ---------------------------------------------------------------------
SELECT orderkey, (SELECT max(name) FROM customers c WHERE c.custkey = o.custkey) AS cname FROM orders o;
SELECT * FROM orders WHERE custkey IN (SELECT custkey FROM customers WHERE nationkey = 3);
SELECT * FROM orders WHERE custkey NOT IN (SELECT custkey FROM customers WHERE acctbal < 0);
SELECT * FROM customers c WHERE EXISTS (SELECT 1 FROM orders o WHERE o.custkey = c.custkey AND o.totalprice > 300000);
SELECT * FROM customers c WHERE NOT EXISTS (SELECT 1 FROM orders o WHERE o.custkey = c.custkey);
SELECT * FROM orders WHERE totalprice > ALL (SELECT totalprice FROM orders WHERE custkey = 1);
SELECT * FROM orders WHERE totalprice = ANY (SELECT max(totalprice) FROM orders GROUP BY custkey);
SELECT * FROM orders WHERE totalprice <> SOME (SELECT 1.0E0);
SELECT * FROM (SELECT custkey, sum(totalprice) AS total FROM orders GROUP BY custkey) t WHERE t.total > 1000000;
WITH big_orders AS (SELECT * FROM orders WHERE totalprice > 100000), by_cust AS (SELECT custkey, count(*) AS cnt FROM big_orders GROUP BY custkey) SELECT c.name, b.cnt FROM by_cust b JOIN customers c ON c.custkey = b.custkey ORDER BY b.cnt DESC LIMIT 10;
WITH t (a, b) AS (VALUES (1, 2), (3, 4)) SELECT a + b FROM t;
WITH RECURSIVE numbers (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM numbers WHERE n < 10) SELECT sum(n) FROM numbers;
WITH RECURSIVE nation_tree (regionkey, nationkey, depth) AS (SELECT regionkey, nationkey, 0 FROM nation WHERE nationkey = 0 UNION ALL SELECT n.regionkey, n.nationkey, t.depth + 1 FROM nation n JOIN nation_tree t ON n.regionkey = t.nationkey WHERE t.depth < 3) SELECT * FROM nation_tree;
SELECT custkey FROM orders UNION SELECT custkey FROM customers;
SELECT custkey FROM orders UNION ALL SELECT custkey FROM customers;
SELECT custkey FROM orders INTERSECT SELECT custkey FROM customers WHERE nationkey = 1;
SELECT custkey FROM customers EXCEPT SELECT custkey FROM orders;
(SELECT custkey FROM orders ORDER BY totalprice DESC LIMIT 5) UNION ALL (SELECT custkey FROM customers ORDER BY acctbal LIMIT 5) ORDER BY 1 LIMIT 8;
SELECT * FROM orders ORDER BY totalprice DESC NULLS LAST OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY;
SELECT * FROM orders ORDER BY orderpriority FETCH FIRST 3 ROWS WITH TIES;
SELECT * FROM orders ORDER BY orderkey OFFSET 100 LIMIT 10;

-- ---------------------------------------------------------------------
-- 12. Выражения, литералы, функции
-- ---------------------------------------------------------------------
SELECT CASE orderstatus WHEN 'O' THEN 'open' WHEN 'F' THEN 'filled' ELSE 'other' END AS s, CASE WHEN totalprice > 1000 THEN 'big' WHEN totalprice > 100 THEN 'mid' END AS sz, IF(totalprice > 0, 1, 0) AS pos, COALESCE(clerk, 'n/a') AS clk, NULLIF(orderstatus, 'O') AS nn FROM orders;
SELECT CAST('123' AS integer), TRY_CAST('abc' AS integer), TRY(1 / 0), CAST(1.5 AS decimal(10, 2)), CAST(NULL AS varchar), CAST('2024-01-01 10:00:00 UTC' AS timestamp(3) with time zone), CAST(ARRAY[1, 2] AS ARRAY(varchar));
SELECT DATE '2024-02-29', TIME '10:11:12.123', TIMESTAMP '2024-02-29 10:11:12.123456', TIMESTAMP '2024-02-29 10:11:12 Europe/Helsinki', INTERVAL '3' DAY, INTERVAL '1-2' YEAR TO MONTH, DECIMAL '1.23', 1.5E0, 123456789012, X'CAFE', U&'\0041\0042', JSON '{"a": [1, 2, 3]}';
SELECT 'a' || 'b', 'abc' LIKE 'a%', 'a_c' LIKE 'a\_c' ESCAPE '\', 5 BETWEEN 1 AND 10, 5 NOT BETWEEN 1 AND 3, 3 IN (1, 2, 3), NULL IS NULL, 1 IS NOT NULL, 1 IS DISTINCT FROM NULL, 1 IS NOT DISTINCT FROM 1, 7 % 3, -5, NOT TRUE, TRUE AND FALSE OR TRUE;
SELECT TIMESTAMP '2024-06-01 12:00:00 UTC' AT TIME ZONE 'Europe/Helsinki', at_timezone(now(), 'UTC'), date_trunc('week', current_date), date_add('day', 7, current_date), date_diff('hour', TIMESTAMP '2024-01-01 00:00:00', TIMESTAMP '2024-01-02 12:00:00'), EXTRACT(YEAR FROM current_date), EXTRACT(DOW FROM current_date), format_datetime(now(), 'yyyy-MM-dd'), date_parse('2024-01-02', '%Y-%m-%d'), from_unixtime(0), to_iso8601(current_date);
SELECT current_date + INTERVAL '1' MONTH, current_timestamp - INTERVAL '2' HOUR, date '2024-01-31' + INTERVAL '1' DAY;
SELECT upper(name), lower(name), length(name), substring(name FROM 1 FOR 3), substr(name, 2), trim(BOTH ' ' FROM name), position('a' IN name), replace(name, 'a', 'b'), split(name, ' '), concat_ws('-', name, phone), regexp_like(name, '^C.*'), regexp_extract(name, '\d+'), format('%s-%05d', name, custkey), lpad(name, 20, '*'), reverse(name), translate(name, 'abc', 'xyz'), normalize(name, NFC) FROM customers;
SELECT abs(-1), ceil(1.2), floor(1.8), round(1.2345, 2), power(2, 10), sqrt(16), mod(10, 3), greatest(1, 2, 3), least(1, 2, 3), rand(), random(10), truncate(1.99), sign(-5), ln(10), log10(100), width_bucket(5.5, 0, 10, 4);
SELECT ARRAY[1, 2, 3][2], MAP(ARRAY['a'], ARRAY[1])['a'], cardinality(ARRAY[1, 2]), element_at(ARRAY[1, 2], 1), contains(ARRAY[1, 2], 2), array_sort(ARRAY[3, 1, 2]), array_distinct(ARRAY[1, 1, 2]), sequence(1, 5), flatten(ARRAY[ARRAY[1], ARRAY[2]]), slice(ARRAY[1, 2, 3, 4], 2, 2), map_keys(MAP(ARRAY['a'], ARRAY[1])), map_concat(MAP(ARRAY[1], ARRAY[2]), MAP(ARRAY[3], ARRAY[4]));
SELECT transform(ARRAY[1, 2, 3], x -> x * 2), filter(ARRAY[1, 2, 3], x -> x > 1), reduce(ARRAY[1, 2, 3], 0, (s, x) -> s + x, s -> s), zip_with(ARRAY[1, 2], ARRAY[3, 4], (a, b) -> a + b), any_match(ARRAY[1, 2], x -> x > 1), all_match(ARRAY[1, 2], x -> x > 0), none_match(ARRAY[1, 2], x -> x > 5), map_filter(MAP(ARRAY[1, 2], ARRAY[10, 20]), (k, v) -> v > 10), transform_values(MAP(ARRAY[1], ARRAY[2]), (k, v) -> v * 2), transform_keys(MAP(ARRAY[1], ARRAY[2]), (k, v) -> k + 1);
SELECT tags, props['browser'] AS browser, geo.lat AS lat, geo.lon AS lon, event_ts AT TIME ZONE 'UTC' AS ts_utc FROM events WHERE contains(tags, 'web') AND event_ts >= TIMESTAMP '2024-01-01 00:00:00 UTC';
SELECT c_row.a, c_row.b.c, c_row.b.d[1], c_map['a'], c_array[1] FROM all_types;

-- UNNEST
SELECT e.event_id, t.tag, t.ord FROM events e CROSS JOIN UNNEST(e.tags) WITH ORDINALITY AS t (tag, ord);
SELECT * FROM UNNEST(ARRAY[1, 2, 3], ARRAY['a', 'b']) AS t (n, s);
SELECT e.event_id, k, v FROM events e, UNNEST(e.props) AS p (k, v);

-- JSON
SELECT json_extract_scalar(payload, '$.page'), json_extract(payload, '$.n'), json_parse(payload), json_format(JSON '[1,2]'), is_json_scalar(json_parse('1')), json_array_length('[1,2,3]') FROM events;
SELECT JSON_VALUE(payload, 'lax $.n' RETURNING integer DEFAULT 0 ON EMPTY), JSON_QUERY(payload, 'lax $' WITH CONDITIONAL ARRAY WRAPPER), JSON_EXISTS(payload, 'lax $.page') FROM events;
SELECT JSON_OBJECT('k' VALUE 1, 'name' VALUE 'x'), JSON_ARRAY(1, 2, 3), JSON_OBJECT(KEY 'a' VALUE NULL NULL ON NULL);
SELECT * FROM JSON_TABLE('{"items":[{"id":1,"name":"a"},{"id":2,"name":"b"}]}', 'lax $.items[*]' COLUMNS (id integer PATH 'lax $.id', name varchar(10) PATH 'lax $.name'));

-- MATCH_RECOGNIZE
SELECT * FROM orders MATCH_RECOGNIZE (
    PARTITION BY custkey
    ORDER BY orderdate
    MEASURES
        match_number() AS match_no,
        classifier() AS cls,
        FIRST(a.totalprice) AS start_price,
        LAST(b.totalprice) AS bottom_price,
        LAST(c.totalprice) AS end_price
    ONE ROW PER MATCH
    AFTER MATCH SKIP PAST LAST ROW
    PATTERN (a b+ c+)
    SUBSET ab = (a, b)
    DEFINE
        b AS totalprice < PREV(totalprice),
        c AS totalprice > PREV(totalprice)
) AS m;
SELECT * FROM orders MATCH_RECOGNIZE (
    PARTITION BY custkey
    ORDER BY orderdate
    MEASURES RUNNING SUM(totalprice) AS running_total, FINAL COUNT(*) AS cnt
    ALL ROWS PER MATCH
    AFTER MATCH SKIP TO NEXT ROW
    PATTERN (up{2,} down?)
    DEFINE up AS totalprice > PREV(totalprice), down AS totalprice < PREV(totalprice)
);

-- Inline SQL routines
WITH FUNCTION double_it(x integer) RETURNS integer RETURN x * 2
SELECT double_it(CAST(orderkey AS integer)) FROM orders LIMIT 5;
WITH FUNCTION price_band(p double) RETURNS varchar RETURN CASE WHEN p > 1000 THEN 'high' ELSE 'low' END
SELECT price_band(totalprice), count(*) FROM orders GROUP BY 1;

-- ---------------------------------------------------------------------
-- 13. EXPLAIN / SHOW / DESCRIBE
-- ---------------------------------------------------------------------
EXPLAIN SELECT * FROM orders WHERE orderkey = 1;
EXPLAIN (TYPE DISTRIBUTED) SELECT custkey, count(*) FROM orders GROUP BY custkey;
EXPLAIN (TYPE LOGICAL, FORMAT JSON) SELECT * FROM orders o JOIN customers c ON o.custkey = c.custkey;
EXPLAIN (TYPE IO, FORMAT JSON) SELECT * FROM orders WHERE orderdate = DATE '1996-01-01';
EXPLAIN (TYPE VALIDATE) SELECT 1;
EXPLAIN ANALYZE SELECT count(*) FROM orders;
EXPLAIN ANALYZE VERBOSE SELECT custkey, sum(totalprice) FROM orders GROUP BY 1;
SHOW CATALOGS LIKE 'ice%';
SHOW TABLES FROM iceberg.parser_test LIKE 'order%';
SHOW COLUMNS FROM orders LIKE 'order%';
DESCRIBE orders;
DESC customers;
SHOW CREATE TABLE orders;
SHOW STATS FOR orders;
SHOW STATS FOR (SELECT * FROM orders WHERE orderdate > DATE '1996-01-01');
SHOW FUNCTIONS LIKE 'array%';
SHOW SESSION LIKE 'iceberg%';

-- ---------------------------------------------------------------------
-- 14. Session, PREPARE, транзакции
-- ---------------------------------------------------------------------
SET SESSION iceberg.compression_codec = 'ZSTD';
SET SESSION join_distribution_type = 'BROADCAST';
SET SESSION query_max_run_time = '10m';
RESET SESSION join_distribution_type;
SET SESSION AUTHORIZATION alice;
RESET SESSION AUTHORIZATION;
SET TIME ZONE 'Europe/Helsinki';
SET TIME ZONE INTERVAL '+02:00' HOUR TO MINUTE;
SET TIME ZONE LOCAL;
SET PATH iceberg.parser_test, system;
PREPARE stmt1 FROM SELECT * FROM orders WHERE orderkey = ? AND totalprice > ?;
DESCRIBE INPUT stmt1;
DESCRIBE OUTPUT stmt1;
EXECUTE stmt1 USING 1, 100.0;
EXECUTE IMMEDIATE 'SELECT count(*) FROM orders WHERE orderkey > ?' USING 100;
DEALLOCATE PREPARE stmt1;
START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY;
SELECT count(*) FROM orders;
COMMIT;
START TRANSACTION;
ROLLBACK;

-- ---------------------------------------------------------------------
-- 15. Security: GRANT / REVOKE / DENY / ROLE
-- ---------------------------------------------------------------------
CREATE ROLE analyst IN iceberg;
GRANT analyst TO USER bob IN iceberg;
GRANT SELECT ON TABLE orders TO USER alice;
GRANT INSERT, UPDATE, DELETE ON orders TO ROLE analyst WITH GRANT OPTION;
GRANT ALL PRIVILEGES ON TABLE customers TO ROLE analyst;
GRANT SELECT ON SCHEMA parser_test TO ROLE analyst;
DENY INSERT ON orders TO USER mallory;
REVOKE GRANT OPTION FOR SELECT ON orders FROM ROLE analyst;
REVOKE SELECT ON TABLE orders FROM USER alice;
SET ROLE analyst IN iceberg;
SET ROLE ALL IN iceberg;
SHOW GRANTS ON TABLE orders;
SHOW GRANTS;
SHOW ROLES IN iceberg;
SHOW ROLE GRANTS IN iceberg;
SHOW CURRENT ROLES IN iceberg;
REVOKE analyst FROM USER bob IN iceberg;
DROP ROLE analyst IN iceberg;

-- ---------------------------------------------------------------------
-- 16. Cleanup (DROP)
-- ---------------------------------------------------------------------
DROP VIEW IF EXISTS v_open_orders2;
DROP VIEW v_cust_totals;
DROP MATERIALIZED VIEW mv_orders_daily2;
DROP TABLE IF EXISTS orders_stage;
DROP TABLE IF EXISTS orders_cte;
DROP TABLE IF EXISTS orders_like;
DROP TABLE IF EXISTS orders_empty;
DROP TABLE IF EXISTS orders_1996;
DROP TABLE IF EXISTS all_types;
DROP TABLE IF EXISTS events;
DROP TABLE IF EXISTS lineitem;
DROP TABLE IF EXISTS region;
DROP TABLE IF EXISTS nation;
DROP TABLE IF EXISTS orders;
DROP TABLE customers;
DROP SCHEMA IF EXISTS iceberg.parser_test;
