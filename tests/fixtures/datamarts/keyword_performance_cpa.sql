SELECT
    keyword,
    match_type,
    SUM(impressions) AS total_impressions,
    SUM(clicks) AS total_clicks,
    SUM(cost) AS total_cost,
    SUM(conversions) AS total_conversions,
    CASE
        WHEN SUM(conversions) > 0 THEN SUM(cost) / SUM(conversions)
        ELSE NULL
    END AS cpa
FROM raw_adtech.keyword_performance
WHERE log_date BETWEEN DATE '2025-09-01' AND DATE '2025-09-26'
GROUP BY keyword, match_type
HAVING SUM(impressions) > 1000;