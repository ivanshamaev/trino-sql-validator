WITH category_sales AS (
    SELECT
        c.category_name,
        p.product_id,
        p.product_name,
        SUM(oi.quantity) AS total_units_sold,
        SUM(oi.quantity * oi.unit_price) AS total_revenue,
        DENSE_RANK() OVER (
            PARTITION BY c.category_name
            ORDER BY SUM(oi.quantity * oi.unit_price) DESC
        ) AS rank_in_category
    FROM raw_ecommerce.order_items oi
    JOIN raw_ecommerce.products p ON oi.product_id = p.product_id
    JOIN raw_ecommerce.categories c ON p.category_id = c.category_id
    GROUP BY c.category_name, p.product_id, p.product_name
)
SELECT
    COALESCE(category_name, 'TOTAL_ALL_CATEGORIES') AS category_name,
    COALESCE(product_name, 'CATEGORY_SUMMARY') AS product_name,
    SUM(total_units_sold) AS units_sold,
    SUM(total_revenue) AS revenue
FROM category_sales
WHERE rank_in_category <= 3
GROUP BY GROUPING SETS (
    (category_name, product_name),
    (category_name),
    ()
);