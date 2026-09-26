SELECT
    picker_id,
    warehouse_zone,
    SHIFT_START AS shift_date,
    COUNT(DISTINCT order_id) AS orders_picked,
    SUM(items_count) AS total_items,
    TIMESTAMP_DIFF(MIN(start_time), MAX(end_time), MINUTE) AS total_shift_minutes,
    SUM(items_count) / NULLIF(TIMESTAMP_DIFF(MIN(start_time), MAX(end_time), MINUTE) / 60.0, 0) AS units_per_hour
FROM lakehouse.wms.picking_tasks
WHERE shift_start >= CURRENT_DATE - INTERVAL '7' DAY
GROUP BY picker_id, warehouse_zone, SHIFT_START;