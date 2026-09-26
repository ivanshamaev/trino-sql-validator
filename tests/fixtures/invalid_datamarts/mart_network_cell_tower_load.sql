SELECT
    tower_id,
    DATE_TRUNC('hour', timestamp) AS hour_bucket,
    MAX_BY(active_connections, timestamp) AS peak_connections,
    AVG(bandwidth_usage_pct) AS avg_bandwidth_load,
    STRING_AGG(DISTINCT failure_code) AS combined_failures
FROM lakehouse.network.cell_logs
WHERE timestamp >= CURRENT_TIMESTAMP - INTERVAL '7' DAY
GROUP BY tower_id, DATE_TRUNC('hour', timestamp);