SELECT
    event_id,
    created_at AS utc_time,
    created_at AT TIME ZONE 'America/New_York' AS est_time,
    created_at AT TIME ZONE 'Europe/Zurich' AS cet_time,
    extract(HOUR FROM created_at AT TIME ZONE 'Europe/Zurich') AS hour_of_day_cet
FROM raw_events.global_events;