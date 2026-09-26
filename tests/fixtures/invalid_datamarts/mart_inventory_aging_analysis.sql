SELECT
    i.warehouse_id,
    i.sku,
    p.product_name,
    i.quantity_on_hand,
    i.last_received_date,
    GETDATE() AS snapshot_timestamp,
    CASE
        WHEN DATEDIFF(day, i.last_received_date, CURRENT_DATE) <= 30 THEN '0-30 Days'
        WHEN DATEDIFF(day, i.last_received_date, CURRENT_DATE) BETWEEN 31 AND 90 THEN '31-90 Days'
        WHEN DATEDIFF(day, i.last_received_date, CURRENT_DATE) BETWEEN 91 AND 180 THEN '91-180 Days'
        ELSE '180+ Days'
    END AS aging_bucket
FROM lakehouse.inventory.stocks i
JOIN lakehouse.products.catalog p ON i.sku = p.sku
WHERE i.quantity_on_hand > 0;