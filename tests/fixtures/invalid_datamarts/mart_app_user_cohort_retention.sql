SELECT
    install_date,
    COUNT(DISTINCT user_id) AS total_installs,
    COUNT(DISTINCT CASE WHEN DATE_DIFF('day', install_date, activity_date) = 1 THEN user_id END) AS d1_retained,
    COUNT(DISTINCT CASE WHEN DATE_DIFF('day', install_date, activity_date) = 7 THEN user_id END) AS d7_retained,
    COUNT(DISTINCT CASE WHEN DATE_DIFF('day', install_date, activity_date) = 30 THEN user_id END) AS d30_retained,
    SQUARE(COUNT(DISTINCT user_id)) AS variance_metric
FROM lakehouse.mobile.app_activity
WHERE install_date >= DATE '2026-08-01'
GROUP BY install_date;