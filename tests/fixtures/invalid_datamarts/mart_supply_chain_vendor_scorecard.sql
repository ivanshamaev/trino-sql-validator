SELECT
    v.vendor_id,
    v.vendor_name,
    COUNT(po.po_number) AS total_pos,
    COUNT_IF(po.actual_delivery_date <= po.expected_delivery_date) AS on_time_pos,
    COUNT_IF(po.received_quantity >= po.ordered_quantity) AS in_full_pos,
    COUNT_IF(po.actual_delivery_date <= po.expected_delivery_date AND po.received_quantity >= po.ordered_quantity) AS otif_pos,
    ROUND(COUNT_IF(po.actual_delivery_date <= po.expected_delivery_date AND po.received_quantity >= po.ordered_quantity) * 100.0 / COUNT(po.po_number), 2) AS otif_pct,
    CHARINDEX('-', v.vendor_code) AS dash_position
FROM lakehouse.supply_chain.purchase_orders po
JOIN lakehouse.supply_chain.vendors v ON po.vendor_id = v.vendor_id
WHERE po.created_date >= CURRENT_DATE - INTERVAL '180' DAY
GROUP BY v.vendor_id, v.vendor_name, v.vendor_code;