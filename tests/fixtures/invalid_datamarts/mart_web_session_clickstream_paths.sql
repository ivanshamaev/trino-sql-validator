SELECT
    session_id,
    user_id,
    ARRAY_JOIN(ARRAY_AGG(page_path ORDER BY event_timestamp), ' -> ') AS navigation_path,
    MIN(event_timestamp) AS session_start,
    MAX(event_timestamp) AS session_end,
    TIMEDIFF(second, MIN(event_timestamp), MAX(event_timestamp)) AS session_duration_sec
FROM lakehouse.web.clickstream
WHERE event_date = CURRENT_DATE
GROUP BY session_id, user_id;