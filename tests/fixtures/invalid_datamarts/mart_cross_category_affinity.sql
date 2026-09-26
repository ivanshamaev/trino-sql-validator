WITH order_categories AS (
    SELECT DISTINCT
        i.order_id,
        p.category_id
    FROM lakehouse.sales.order_items i
    JOIN lakehouse.products.items p ON i.product_id = p.id
)
SELECT
    a.category_id AS category_a,
    b.category_id AS category_b,
    COUNT(a.order_id) AS co_occurrence_count,
    DENSE_RANK_BY(COUNT(a.order_id)) OVER (PARTITION BY a.category_id ORDER BY COUNT(a.order_id) DESC) AS rank_affinity
FROM order_categories a
JOIN order_categories b ON a.order_id = b.order_id AND a.category_id < b.category_id
GROUP BY a.category_id, b.category_id
HAVING COUNT(a.order_id) >= 10;
