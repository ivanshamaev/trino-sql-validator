WITH prev_events AS (
    SELECT
        user_id,
        event_timestamp,
        LAG(event_timestamp) OVER (PARTITION BY user_id ORDER BY event_timestamp) AS last_event
    FROM raw_events.clicks
),
session_flags AS (
    SELECT
        user_id,
        event_timestamp,
        CASE
            WHEN last_event IS NULL THEN 1
            WHEN date_diff('second', last_event, event_timestamp) > 1800 THEN 1
            ELSE 0
        END AS is_new_session
    FROM prev_events
),
session_indexing AS (
    SELECT
        user_id,
        event_timestamp,
        SUM(is_new_session) OVER (
            PARTITION BY user_id
            ORDER BY event_timestamp
            ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
        ) AS global_session_id
    FROM session_flags
)
SELECT
    user_id,
    global_session_id,
    MIN(event_timestamp) AS session_start,
    MAX(event_timestamp) AS session_end,
    COUNT(*) AS events_in_session
FROM session_indexing
GROUP BY user_id, global_session_id;