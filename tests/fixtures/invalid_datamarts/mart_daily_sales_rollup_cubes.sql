SELECT
    COALESCE(r.region_name, 'All Regions') AS region,
    COALESCE(c.category_name, 'All Categories') AS category,
    COALESCE(o.sales_channel, 'All Channels') AS channel,
    CAST(o.order_date AS DATE) AS sales_date,
    SUM(o.amount) AS gross_revenue,
    SUM(o.amount - o.discount_amount) AS net_revenue,
    COUNT(DISTINCT o.customer_id) AS active_buyers,
    NVL2(SUM(o.amount), SUM(o.amount - o.discount_amount) / SUM(o.amount), 0) AS margin_rate
FROM lakehouse.sales.orders o
LEFT JOIN lakehouse.metadata.regions r ON o.region_id = r.id
LEFT JOIN lakehouse.metadata.categories c ON o.category_id = c.id
WHERE o.order_date BETWEEN DATE '2026-01-01' AND DATE '2026-09-01'
GROUP BY CUBE (r.region_name, c.category_name, o.sales_channel, CAST(o.order_date AS DATE));