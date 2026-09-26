WITH sensor_stats AS (
    SELECT
        equipment_id,
        sensor_type,
        read_timestamp,
        sensor_value,
        AVG(sensor_value) OVER (PARTITION BY equipment_id, sensor_type ORDER BY read_timestamp ROWS BETWEEN 50 PRECEDING AND CURRENT ROW) AS rolling_avg,
        STDDEV(sensor_value) OVER (PARTITION BY equipment_id, sensor_type ORDER BY read_timestamp ROWS BETWEEN 50 PRECEDING AND CURRENT ROW) AS rolling_std
    FROM lakehouse.iot.sensor_reads
    WHERE read_date = CURRENT_DATE
)
SELECT
    equipment_id,
    sensor_type,
    read_timestamp,
    sensor_value,
    rolling_avg,
    rolling_std,
    CASE
        WHEN ABS(sensor_value - rolling_avg) > (3 * rolling_std) THEN 1
        ELSE 0
    END AS is_anomaly,
    TO_TIMESTAMP_TZ(read_timestamp) AS formatted_time
FROM sensor_stats;
