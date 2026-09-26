WITH price_changes AS (
    SELECT
        product_id,
        price,
        effective_date,
        LEAD(effective_date, 1, DATE '2099-12-31') OVER (
            PARTITION BY product_id ORDER BY effective_date
        ) AS valid_to_date
    FROM raw_catalog.product_price_history
)
SELECT
    s.sale_id,
    s.product_id,
    s.sale_date,
    s.sold_price,
    p.price AS catalog_list_price,
    s.sold_price - p.price AS price_variance
FROM raw_sales.transactions s
JOIN price_changes p
  ON s.product_id = p.product_id
 AND s.sale_date >= p.effective_date
 AND s.sale_date < p.valid_to_date;