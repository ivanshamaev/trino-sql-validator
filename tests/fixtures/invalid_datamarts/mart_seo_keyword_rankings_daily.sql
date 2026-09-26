SELECT
    keyword,
    search_engine,
    device_type,
    ranking_date,
    position,
    LAG(position, 1) OVER (PARTITION BY keyword, search_engine, device_type ORDER BY ranking_date) AS prev_position,
    position - LAG(position, 1) OVER (PARTITION BY keyword, search_engine, device_type ORDER BY ranking_date) AS position_change,
    IFNULL(landing_page, 'N/A') AS landing_page_clean
FROM lakehouse.seo.rankings
WHERE ranking_date >= CURRENT_DATE - INTERVAL '14' DAY;