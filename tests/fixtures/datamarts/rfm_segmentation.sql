CREATE OR REPLACE TABLE analytics.mart_rfm_segments AS
WITH user_orders AS (
    SELECT
        user_id,
        MAX(order_timestamp) AS last_order_dt,
        COUNT(DISTINCT order_id) AS total_orders,
        SUM(total_amount) AS total_spend
    FROM raw_ecommerce.orders
    WHERE order_status = 'COMPLETED'
      AND order_timestamp >= DATE '2025-01-01'
    GROUP BY user_id
),
rfm_scores AS (
    SELECT
        user_id,
        date_diff('day', last_order_dt, CURRENT_DATE) AS recency_days,
        total_orders AS frequency,
        total_spend AS monetary,
        NTILE(5) OVER (ORDER BY date_diff('day', last_order_dt, CURRENT_DATE) DESC) AS r_score,
        NTILE(5) OVER (ORDER BY total_orders ASC) AS f_score,
        NTILE(5) OVER (ORDER BY total_spend ASC) AS m_score
    FROM user_orders
)
SELECT
    user_id,
    recency_days,
    frequency,
    monetary,
    r_score,
    f_score,
    m_score,
    CASE
        WHEN r_score >= 4 AND f_score >= 4 AND m_score >= 4 THEN 'Champions'
        WHEN r_score >= 3 AND f_score >= 3 THEN 'Loyal Customers'
        WHEN r_score <= 2 AND f_score >= 4 THEN 'At Risk'
        WHEN r_score <= 2 AND f_score <= 2 THEN 'Hibernating'
        ELSE 'Others'
    END AS customer_segment
FROM rfm_scores;