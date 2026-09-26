CREATE TABLE analytics.mart_daily_sales_summary
WITH (
    format = 'PARQUET',
    partitioning = ARRAY['order_date'],
    location = 's3a://my-bucket/analytics/mart_daily_sales_summary/'
) AS
SELECT
    CAST(order_timestamp AS DATE) AS order_date,
    store_id,
    currency_code,
    COUNT(DISTINCT order_id) AS total_orders,
    SUM(total_amount) AS total_revenue,
    AVG(total_amount) AS avg_order_value
FROM raw_ecommerce.orders
WHERE order_status = 'COMPLETED'
GROUP BY CAST(order_timestamp AS DATE), store_id, currency_code;