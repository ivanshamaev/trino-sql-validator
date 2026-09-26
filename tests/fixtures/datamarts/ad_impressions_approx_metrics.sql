SELECT
    advertiser_id,
    date_trunc('day', event_timestamp) AS event_date,
    COUNT(*) AS exact_impressions,
    approx_distinct(user_id, 0.01) AS approx_unique_users,
    approx_percentile(latency_ms, 0.50) AS median_latency_ms,
    approx_percentile(latency_ms, 0.95) AS p95_latency_ms,
    approx_percentile(latency_ms, 0.99) AS p99_latency_ms
FROM raw_adtech.ad_impressions
GROUP BY 1, 2;