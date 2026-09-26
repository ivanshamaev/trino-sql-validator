SELECT
    p.package_id,
    p.tracking_number,
    checkpoint.idx AS checkpoint_step,
    checkpoint.val.location_code AS location,
    checkpoint.val.status AS status_code,
    from_unixtime(checkpoint.val.timestamp / 1000) AS status_time,
    LEAD(from_unixtime(checkpoint.val.timestamp / 1000)) OVER (
        PARTITION BY p.package_id
        ORDER BY checkpoint.idx
    ) AS next_checkpoint_time
FROM raw_logistics.packages p
CROSS JOIN UNNEST(p.checkpoint_history) WITH ORDINALITY AS checkpoint(val, idx)
WHERE p.created_date >= DATE '2025-01-01';