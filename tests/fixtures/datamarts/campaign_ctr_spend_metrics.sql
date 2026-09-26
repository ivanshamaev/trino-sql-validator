SELECT
    c.campaign_id,
    c.campaign_name,
    SUM(i.impressions_count) AS total_impressions,
    SUM(cl.clicks_count) AS total_clicks,
    SUM(cl.clicks_count) * 1.0 / NULLIF(SUM(i.impressions_count), 0) AS ctr,
    SUM(s.spend_amount) AS total_spend,
    (SUM(s.spend_amount) / NULLIF(SUM(i.impressions_count), 0)) * 1000.0 AS ecpm
FROM raw_adtech.campaigns c
LEFT JOIN (
    SELECT campaign_id, COUNT(*) AS impressions_count
    FROM raw_adtech.impressions GROUP BY 1
) i ON c.campaign_id = i.campaign_id
LEFT JOIN (
    SELECT campaign_id, COUNT(*) AS clicks_count
    FROM raw_adtech.clicks GROUP BY 1
) cl ON c.campaign_id = cl.campaign_id
LEFT JOIN raw_adtech.spend s ON c.campaign_id = s.campaign_id
GROUP BY c.campaign_id, c.campaign_name;