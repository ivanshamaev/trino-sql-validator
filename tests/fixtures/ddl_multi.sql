CREATE TABLE IF NOT EXISTS fact_orders (
    order_id BIGINT,
    customer_id BIGINT,
    amount DECIMAL(18, 2),
    created_at TIMESTAMP
)
WITH (format = 'ORC', partitioned_by = ARRAY['created_date']);

DROP TABLE IF EXISTS staging_tmp;

INSERT INTO fact_orders (order_id, customer_id, amount, created_at)
SELECT order_id, customer_id, amount, created_at
FROM staging_orders;