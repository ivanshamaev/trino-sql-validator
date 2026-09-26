SELECT
    o.warehouse_id,
    o.carrier_code,
    DATE_TRUNC('month', o.created_at) AS order_month,
    COUNT(o.order_id) AS total_orders,
    AVG(DATE_DIFF('hour', o.created_at, o.shipped_at)) AS avg_fulfillment_hours,
    PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY DATE_DIFF('hour', o.created_at, o.shipped_at)) AS p95_fulfillment_hours,
    SUM(CASE WHEN DATE_DIFF('hour', o.created_at, o.delivered_at) > 48 THEN 1 ELSE 0 END) AS sla_breached_orders,
    DECODE(o.priority_code, 1, 'High', 2, 'Medium', 3, 'Low', 'Standard') AS priority_label
FROM lakehouse.logistics.orders o
WHERE o.created_at >= TIMESTAMP '2026-01-01 00:00:00'
GROUP BY o.warehouse_id, o.carrier_code, DATE_TRUNC('month', o.created_at);