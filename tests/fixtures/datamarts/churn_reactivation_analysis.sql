WITH user_activity_gaps AS (
    SELECT
        user_id,
        order_timestamp,
        LAG(order_timestamp) OVER (PARTITION BY user_id ORDER BY order_timestamp) AS prev_order_dt,
        date_diff('day',
            LAG(order_timestamp) OVER (PARTITION BY user_id ORDER BY order_timestamp),
            order_timestamp
        ) AS days_since_last_order
    FROM raw_ecommerce.orders
),
churn_events AS (
    SELECT
        user_id,
        order_timestamp,
        days_since_last_order,
        CASE WHEN days_since_last_order > 90 THEN 1 ELSE 0 END AS is_reactivation
    FROM user_activity_gaps
)
SELECT
    date_trunc('month', order_timestamp) AS month_period,
    COUNT(DISTINCT user_id) AS active_users,
    SUM(is_reactivation) AS reactivated_users,
    AVG(days_since_last_order) AS avg_inter_order_days
FROM churn_events
GROUP BY 1
ORDER BY 1 DESC;