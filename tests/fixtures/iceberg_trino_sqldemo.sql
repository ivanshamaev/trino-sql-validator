SET SESSION skip_results_cache=true;

--- ICEBERG ------------
-- DROP SCHEMA IF EXISTS victorc_iceberg;

CREATE SCHEMA IF NOT EXISTS iceberg_glue.victorc_iceberg WITH (location = 's3://victorc-data/iceberg/');
SHOW CREATE SCHEMA "iceberg_glue"."victorc_iceberg";

USE iceberg_glue.victorc_iceberg;
 
CREATE OR REPLACE TABLE nation AS SELECT * FROM tpch.sf1.nation;

CREATE OR REPLACE TABLE orders WITH (partitioning = ARRAY['year(orderdate)']) AS SELECT * FROM tpch.sf1.orders;

SELECT orderdate FROM orders WHERE orderdate>=date('1992-11-01') and orderdate<date('1993-01-01');

DROP TABLE IF EXISTS customer;

CREATE OR REPLACE TABLE customer
WITH (
  partitioning = ARRAY['mktsegment'],
  format='parquet',
  format_version = 3,
  merge_mode = 'merge-on-read'
-- merge_mode = 'copy-on-write'
-- location = 's3://my-bucket/a/path/'
-- sorted_by = ARRAY['order_date']
) AS
SELECT
    c.custkey,
    c.name,
    c.mktsegment,
    ROUND(c.acctbal) as account_balance,
    n.name as nation
FROM
    postgresql.demo.customer c
    join oracle.demo.nation n on c.nationkey = n.nationkey
    join snowflake_parallel.demo.region r on r.regionkey = n.regionkey
WHERE r.name = 'EUROPE'
LIMIT 500;

SHOW CREATE TABLE customer;

SELECT * FROM customer ASC ORDER BY name;

------- Get Columns Statistics for Trino CBO -----------------------------------------------------------

ANALYZE customer;
SHOW STATS FOR customer;

--------- Views & Materialized Views -----------------

CREATE OR REPLACE VIEW my_view
AS SELECT * FROM tpch.sf1.nation;
SELECT * FROM my_view;

CREATE OR REPLACE MATERIALIZED VIEW my_mv
WITH (refresh_schedule = '30 04 * * 0')
AS SELECT * FROM tpch.sf1.nation;
REFRESH MATERIALIZED VIEW my_mv;
SELECT * FROM my_mv;

--------- Metadata Tables & Columns -----------------------------------------------------------------------------

SELECT
 custkey,
 "$row_id",
 "$last_updated_sequence_number",
 "$partition" ,
 "$path" as file,
 "$file_modified_time" as filedatetime
FROM customer;

SELECT * FROM "customer$properties";
SELECT * FROM "customer$snapshots";
SELECT * FROM "customer$history";
SELECT * FROM "customer$manifests";
SELECT * FROM "customer$partitions";
SELECT * FROM "customer$files";
SELECT * FROM "customer$refs";
SELECT * FROM "customer$entries";
SELECT * FROM "customer$metadata_log_entries"; 


CREATE OR REPLACE VIEW curr_ver_dets AS
SELECT concat_ws(' > ', r.name, r.type) AS curr_ver,
       date_format(s.committed_at, '%Y/%m/%d-%T') AS committed_at,
       s.snapshot_id, s.parent_id, h.is_current_ancestor, s.operation
  FROM "customer$snapshots" AS s
  JOIN "customer$history" AS h
    ON (s.snapshot_id = h.snapshot_id)
  LEFT JOIN "customer$refs" AS r
    ON (h.snapshot_id = r.snapshot_id)
ORDER BY s.committed_at;

select * from curr_ver_dets;


------------- INSERT / UPDATE / DELETE / MERGE  ------------------------------------------------------------------

SELECT * FROM customer ORDER BY name;

INSERT INTO customer (custkey, name, mktsegment, account_balance, nation)
VALUES (2001 , 'COMMANDER BUN BUN', 'SQLENGINE', 1, 'FRANCE'),
(2002 , 'COMMANDER BUN BUN', 'SQLENGINE', 2, 'FRANCE'),
(2003 , 'COMMANDER BUN BUN', 'SQLENGINE', 3, 'FRANCE');

SELECT * FROM customer ORDER BY name; 

---- Row-level changes between two versions of an Iceberg table.

SELECT * FROM "customer$snapshots" ORDER BY committed_at ASC;

SELECT
    *
FROM
    TABLE(
            system.table_changes(
                    schema_name => 'victorc_iceberg',
                    table_name => 'customer',
                    start_snapshot_id => 8971812331224479488,
                    end_snapshot_id => 5174658899552221530
            )
    )
ORDER BY _change_ordinal ASC;

------------------------ UPDATE ----------------

UPDATE customer SET account_balance = 1000 WHERE custkey = 2001;
SELECT * FROM customer ORDER BY name;
SELECT * FROM "customer$snapshots" ORDER BY committed_at DESC;

----- ROW LINEAGE ---------

SELECT name, custkey,"$row_id", "$last_updated_sequence_number" FROM customer ORDER BY name;

DELETE FROM customer WHERE custkey IN (2001,2002,2003);
SELECT * FROM customer ORDER BY name;
SELECT * FROM "customer$snapshots" ORDER BY committed_at DESC;

----- DELETION VECTOR ---------

SELECT file_path, file_format FROM "customer$files";


-- MERGE INTO s3lakehouse.blog.customer_base AS b
-- USING s3lakehouse.blog.customer_land AS l
-- ON (b.custkey = l.custkey)
-- WHEN MATCHED and b.name != l.name
-- THEN UPDATE
-- SET name = l.name ,
--     state = l.state,
--     zip = l.zip,
--     cust_since = l.cust_since
-- WHEN NOT MATCHED
--       THEN INSERT (custkey, name, state, zip, cust_since,last_update_dt)
--             VALUES(l.custkey, l.name, l.state, l.zip, l.cust_since,l.last_update_dt);

------- ALTER / Schema Evolution --------------------------------------------------------------------------------

SELECT * FROM "customer$snapshots" ORDER BY committed_at DESC;
SELECT * FROM customer ORDER BY name;

----- DEFAULT COLUMN VALUE ---------

ALTER TABLE customer ADD COLUMN phone varchar DEFAULT '+33606060606';

INSERT INTO customer (custkey,name,mktsegment,account_balance,nation) 
VALUES (200000, 'COMMANDER BUN BUN', 'SQLENGINE', 1, 'FRANCE');

SELECT * FROM customer ORDER BY name;

----- VARIANT / JSON TYPE ---------

ALTER TABLE customer ADD COLUMN message JSON;
INSERT INTO customer (custkey,name,mktsegment,account_balance,nation,message)
VALUES (200000, 'COMMANDER BUN BUN', 'SQLENGINE', 1, 'FRANCE',JSON '{"company": "Starburst"}');

SELECT * FROM customer ORDER BY name;

ALTER TABLE customer DROP COLUMN message;

----- NANOSECOND ---------
 
ALTER TABLE customer ADD COLUMN nanos TIMESTAMP(9);
INSERT INTO customer (custkey,name,mktsegment,account_balance,nation,nanos)
VALUES (200000, 'COMMANDER BUN BUN', 'SQLENGINE', 1, 'FRANCE',TIMESTAMP '2025-08-21 12:34:56.123456789');

SELECT * FROM customer ORDER BY name;

------- Partition Evolution ---------------------------------------------------------------------------------------

ALTER TABLE customer SET PROPERTIES partitioning = ARRAY['mktsegment', 'custkey'];

INSERT INTO customer (custkey,name,mktsegment,account_balance,nation) VALUES (200001, 'TRINO', 'SQLENGINE', 1, 'FRANCE');
INSERT INTO customer (custkey,name,mktsegment,account_balance,nation) VALUES (200002, 'STARBURST', 'SQLENGINE', 2, 'FRANCE');

SELECT * FROM "customer$snapshots" ORDER BY committed_at DESC;
SELECT * FROM "customer$partitions";

ALTER TABLE customer SET PROPERTIES partitioning = ARRAY['nation'];

INSERT INTO customer (custkey,name,mktsegment,account_balance,nation) VALUES (200001, 'TRINO', 'SQLENGINE', 1, 'ITALY');
INSERT INTO customer (custkey,name,mktsegment,account_balance,nation) VALUES (200002, 'STARBURST', 'SQLENGINE', 2, 'SPAIN');

SELECT * FROM "customer$snapshots" ORDER BY committed_at DESC;
SELECT * FROM "customer$partitions";

SELECT * FROM "customer$files";

------- Time Travel / Snapshots -----------------------------------------------------------------------------------------

SELECT * FROM "customer$snapshots" ORDER BY committed_at ASC;

SELECT * FROM customer where mktsegment='SQLENGINE';
SELECT * FROM customer FOR VERSION AS OF 5174658899552221530 where mktsegment='SQLENGINE'  ORDER BY name;

CALL system.rollback_to_snapshot('victorc_iceberg', 'customer', 5174658899552221530);
SELECT * FROM customer where mktsegment='SQLENGINE';

select * from curr_ver_dets;


-------------------------------------------------------
--------------- BRANCHING -----------------------------
-------------------------------------------------------

DELETE FROM customer where mktsegment='SQLENGINE';
INSERT INTO customer (custkey, name, mktsegment, account_balance, nation) VALUES (200000, 'TRINO', 'SQLENGINE', 10000, 'FRANCE');
INSERT INTO customer (custkey, name, mktsegment, account_balance, nation) VALUES (200001, 'STARBURST', 'SQLENGINE', 10000, 'FRANCE');

SELECT * FROM customer where mktsegment='SQLENGINE';

DROP BRANCH IF EXISTS dev IN TABLE customer;
CREATE BRANCH IF NOT EXISTS dev IN TABLE customer;
SHOW BRANCHES FROM TABLE customer;

INSERT INTO customer @ dev(custkey,name,mktsegment,account_balance,nation) VALUES (200005, 'SPARK', 'SQLENGINE', 1, 'ITALY');
INSERT INTO customer @ dev(custkey,name,mktsegment,account_balance,nation) VALUES (200006, 'CLICKHOUSE', 'SQLENGINE', 2, 'SPAIN');
DELETE FROM customer @ dev WHERE custkey = 200001;
UPDATE customer @ dev SET account_balance = 0 WHERE custkey = 200000;

SELECT * FROM customer FOR VERSION AS OF 'dev' where mktsegment='SQLENGINE';

SELECT * FROM customer FOR VERSION AS OF 'main' where mktsegment='SQLENGINE';

SELECT * FROM customer where mktsegment='SQLENGINE';

ALTER BRANCH main IN TABLE customer FAST FORWARD TO dev;

SELECT * FROM customer where mktsegment='SQLENGINE';

select * from curr_ver_dets;


------- Optimize / Compaction - Vacuum / Cleaning snapshots and orphan files --------------------------

SELECT * FROM "customer$files";

ALTER TABLE customer EXECUTE expire_snapshots(retention_threshold => '7d');
ALTER TABLE customer EXECUTE remove_orphan_files(retention_threshold => '7d');

ALTER TABLE customer EXECUTE optimize(file_size_threshold => '100MB');

SELECT * FROM "customer$files";

ALTER TABLE customer EXECUTE optimize 
WHERE "$file_modified_time" > CAST(now() - INTERVAL '2' DAY AS DATE);

--------------  Register Table ------------------------------------------

DROP TABLE IF EXISTS new_customer;

SELECT * FROM "customer$properties";

CALL system.register_table(
  schema_name => 'victorc_iceberg', 
  table_name => 'new_customer', 
  table_location => 's3://victorc-data/iceberg/customer-ddaf4ed693ae48c686e1722aa0ce12dd');

SELECT * FROM new_customer;


------- Federation with PostgreSQL data --------------------------------------------------------------------------------

SELECT
    c.nation as nation,
    round(sum(o.totalprice)) as total_price
FROM
    customer c
    join postgresql.demo.orders o on c.custkey = o.custkey
WHERE 
    c.mktsegment='AUTOMOBILE'
GROUP BY c.nation
ORDER BY total_price;
