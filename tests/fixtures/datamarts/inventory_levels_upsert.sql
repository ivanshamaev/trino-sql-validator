MERGE INTO analytics.mart_inventory_levels target
USING (
    SELECT
        warehouse_id,
        product_id,
        current_stock,
        reserved_stock,
        CURRENT_TIMESTAMP AS last_updated_at
    FROM raw_logistics.stage_inventory
) source
ON (target.warehouse_id = source.warehouse_id AND target.product_id = source.product_id)
WHEN MATCHED THEN
    UPDATE SET
        current_stock = source.current_stock,
        reserved_stock = source.reserved_stock,
        last_updated_at = source.last_updated_at
WHEN NOT MATCHED THEN
    INSERT (warehouse_id, product_id, current_stock, reserved_stock, last_updated_at)
    VALUES (source.warehouse_id, source.product_id, source.current_stock, source.reserved_stock, source.last_updated_at);