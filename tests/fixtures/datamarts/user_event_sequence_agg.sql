SELECT
    user_id,
    array_agg(event_name ORDER BY event_timestamp ASC) AS sequence_of_events,
    array_distinct(array_agg(category_id)) AS unique_categories_viewed,
    sequence(
        CAST(MIN(event_timestamp) AS DATE),
        CAST(MAX(event_timestamp) AS DATE),
        INTERVAL '1' DAY
    ) AS active_days_range
FROM raw_events.user_actions
GROUP BY user_id;