WITH vehicle_status_changes AS (
    SELECT
        vehicle_id,
        status,
        event_timestamp,
        LAG(status) OVER (PARTITION BY vehicle_id ORDER BY event_timestamp) AS prev_status,
        LAG(event_timestamp) OVER (PARTITION BY vehicle_id ORDER BY event_timestamp) AS prev_event_timestamp
    FROM raw_fleet.telematics
),
downtime_periods AS (
    SELECT
        vehicle_id,
        prev_event_timestamp AS downtime_start,
        event_timestamp AS downtime_end,
        date_diff('minute', prev_event_timestamp, event_timestamp) AS duration_minutes
    FROM vehicle_status_changes
    WHERE status = 'ACTIVE' AND prev_status = 'MAINTENANCE'
)
SELECT
    vehicle_id,
    date_trunc('month', downtime_start) AS month_bucket,
    COUNT(*) AS maintenance_events_count,
    SUM(duration_minutes) AS total_downtime_minutes,
    AVG(duration_minutes) AS avg_downtime_minutes
FROM downtime_periods
GROUP BY 1, 2;