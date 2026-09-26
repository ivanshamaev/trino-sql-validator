SELECT
    agent_id,
    queue_name,
    CAST(call_start_time AS DATE) AS shift_date,
    COUNT(call_id) AS total_calls_handled,
    AVG(talk_duration_sec + hold_duration_sec + wrapup_duration_sec) AS avg_handle_time_sec,
    PERCENTILE(csat_score, 0.5) AS median_csat,
    SUM(CASE WHEN is_first_contact_resolved THEN 1 ELSE 0 END) * 100.0 / COUNT(call_id) AS fcr_pct
FROM lakehouse.callcenter.calls
WHERE call_start_time >= CURRENT_DATE - INTERVAL '30' DAY
GROUP BY agent_id, queue_name, CAST(call_start_time AS DATE);