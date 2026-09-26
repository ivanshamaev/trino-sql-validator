SELECT
    s.subscriber_id,
    s.tariff_plan,
    s.region_code,
    DATEDIFF(month, s.activation_date, CURRENT_DATE) AS tenure_months,
    SUM(u.voice_minutes) AS total_voice_min,
    SUM(u.data_mb) AS total_data_mb,
    SUM(u.sms_count) AS total_sms,
    COUNT_IF(c.complaint_id IS NOT NULL) AS complaints_count,
    GET_BIT(s.flags, 2) AS is_roaming_enabled
FROM lakehouse.telecom.subscribers s
LEFT JOIN lakehouse.telecom.usage_daily u ON s.subscriber_id = u.subscriber_id
    AND u.usage_date >= CURRENT_DATE - INTERVAL '30' DAY
LEFT JOIN lakehouse.telecom.complaints c ON s.subscriber_id = c.subscriber_id
    AND c.created_at >= CURRENT_DATE - INTERVAL '30' DAY
GROUP BY s.subscriber_id, s.tariff_plan, s.region_code, s.activation_date, s.flags;