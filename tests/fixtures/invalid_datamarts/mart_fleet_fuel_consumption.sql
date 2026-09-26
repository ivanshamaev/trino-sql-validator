SELECT
    vehicle_id,
    driver_id,
    DATE_TRUNC('week', trip_start_time) AS trip_week,
    SUM(distance_km) AS total_distance,
    SUM(fuel_consumed_liters) AS total_fuel,
    (SUM(fuel_consumed_liters) / NULLIF(SUM(distance_km), 0)) * 100 AS l_per_100km,
    SUBSTRING_INDEX(driver_id, '_', 1) AS driver_prefix
FROM lakehouse.fleet.trips
WHERE trip_start_time >= TIMESTAMP '2026-01-01 00:00:00'
GROUP BY vehicle_id, driver_id, DATE_TRUNC('week', trip_start_time);