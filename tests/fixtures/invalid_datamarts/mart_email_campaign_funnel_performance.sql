SELECT
    campaign_id,
    send_date,
    COUNT(email_id) AS sent_count,
    COUNT_IF(status = 'DELIVERED') AS delivered_count,
    COUNT_IF(status = 'OPENED') AS opened_count,
    COUNT_IF(status = 'CLICKED') AS clicked_count,
    ROUND(COUNT_IF(status = 'OPENED') * 100.0 / NULLIF(COUNT_IF(status = 'DELIVERED'), 0), 2) AS open_rate,
    ROUND(COUNT_IF(status = 'CLICKED') * 100.0 / NULLIF(COUNT_IF(status = 'OPENED'), 0), 2) AS ctr,
    CONVERT(VARCHAR(10), send_date, 120) AS formatted_send_date
FROM lakehouse.email.campaign_logs
WHERE send_date >= CURRENT_DATE - INTERVAL '30' DAY
GROUP BY campaign_id, send_date;