WITH order_products AS (
    SELECT DISTINCT
        order_id,
        product_id
    FROM raw_ecommerce.order_items
),
product_pairs AS (
    SELECT
        op1.product_id AS product_a,
        op2.product_id AS product_b,
        COUNT(DISTINCT op1.order_id) AS pair_frequency
    FROM order_products op1
    JOIN order_products op2
      ON op1.order_id = op2.order_id
     AND op1.product_id < op2.product_id
    GROUP BY op1.product_id, op2.product_id
    HAVING COUNT(DISTINCT op1.order_id) >= 10
)
SELECT
    pa.product_name AS item_a,
    pb.product_name AS item_b,
    pp.pair_frequency,
    pp.pair_frequency * 100.0 / (SELECT COUNT(DISTINCT order_id) FROM raw_ecommerce.order_items) AS support_pct
FROM product_pairs pp
JOIN raw_ecommerce.products pa ON pp.product_a = pa.product_id
JOIN raw_ecommerce.products pb ON pp.product_b = pb.product_id
ORDER BY pp.pair_frequency DESC
LIMIT 50;