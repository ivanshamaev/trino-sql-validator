WITH customer_activity AS (
    SELECT
        user_id,
        MAX(order_date) AS last_order_date,
        COUNT(DISTINCT order_id) AS total_orders,
        SUM(amount) AS total_spent,
        AVG(amount) AS avg_order_value
    FROM lakehouse.sales.orders
    WHERE order_date >= CURRENT_DATE - INTERVAL '365' DAY
      AND order_status IN ('COMPLETED', 'DELIVERED')
    GROUP BY user_id
),
rfm_scores AS (
    SELECT
        user_id,
        DATEDIFF(day, last_order_date, CURRENT_DATE) AS recency_days,
        NTILE(5) OVER (ORDER BY last_order_date ASC) AS r_score,
        NTILE(5) OVER (ORDER BY total_orders ASC) AS f_score,
        NTILE(5) OVER (ORDER BY total_spent ASC) AS m_score
    FROM customer_activity
)
SELECT
    user_id,
    recency_days,
    r_score,
    f_score,
    m_score,
    CONCAT(CAST(r_score AS VARCHAR), CAST(f_score AS VARCHAR), CAST(m_score AS VARCHAR)) AS rfm_segment,
    CASE
        WHEN r_score >= 4 AND f_score >= 4 AND m_score >= 4 THEN 'Champions'
        WHEN f_score >= 3 AND m_score >= 3 THEN 'Loyal Customers'
        WHEN r_score <= 2 AND f_score >= 3 THEN 'At Risk'
        ELSE 'Regular'
    END AS customer_category
FROM rfm_scores;