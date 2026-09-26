SELECT
    content_id,
    device_os,
    isp_provider,
    COUNT(DISTINCT session_id) AS total_sessions,
    SUM(buffer_events_count) AS total_rebuffers,
    AVG(avg_bitrate_kbps) AS mean_bitrate,
    APPROX_PERCENTILE(startup_time_ms, 0.95) AS p95_startup_time,
    INSTR(stream_url, 'hls') AS is_hls_stream,
    STR_TO_DATE(session_start_str, '%Y-%m-%d %H:%i:%s') AS session_start_dt
FROM lakehouse.streaming.qos_events
WHERE event_date = CURRENT_DATE
GROUP BY content_id, device_os, isp_provider, INSTR(stream_url, 'hls'), STR_TO_DATE(session_start_str, '%Y-%m-%d %H:%i:%s');