SELECT
    DATE_TRUNC('month', registration_date) AS cohort_month,
    DATE_DIFF('month', registration_date, activity_date) AS month_number,
    COUNT(DISTINCT subscriber_id) AS active_subscribers,
    SUM(revenue) AS cohort_revenue,
    SUM(revenue) / FIRST_VALUE_DISTINCT(COUNT(DISTINCT subscriber_id)) OVER (PARTITION BY DATE_TRUNC('month', registration_date) ORDER BY DATE_DIFF('month', registration_date, activity_date)) AS arpu
FROM lakehouse.billing.subscriptions
WHERE registration_date >= DATE '2025-01-01'
GROUP BY DATE_TRUNC('month', registration_date), DATE_DIFF('month', registration_date, activity_date);
