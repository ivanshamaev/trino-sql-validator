CREATE VIEW analytics.v_cohort_ltv_analysis AS
WITH user_cohorts AS (
    SELECT
        user_id,
        date_trunc('month', registration_date) AS cohort_month
    FROM raw_ecommerce.users
),
monthly_revenue AS (
    SELECT
        o.user_id,
        date_trunc('month', o.order_timestamp) AS activity_month,
        SUM(o.total_amount) AS revenue
    FROM raw_ecommerce.orders o
    WHERE o.order_status = 'COMPLETED'
    GROUP BY 1, 2
)
SELECT
    c.cohort_month,
    date_diff('month', c.cohort_month, r.activity_month) AS month_number,
    COUNT(DISTINCT c.user_id) AS cohort_size,
    COUNT(DISTINCT r.user_id) AS active_users,
    SUM(r.revenue) AS period_revenue,
    SUM(SUM(r.revenue)) OVER (
        PARTITION BY c.cohort_month
        ORDER BY date_diff('month', c.cohort_month, r.activity_month)
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    ) AS cumulative_ltv_revenue,
    SUM(SUM(r.revenue)) OVER (
        PARTITION BY c.cohort_month
        ORDER BY date_diff('month', c.cohort_month, r.activity_month)
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    ) / CAST(COUNT(DISTINCT c.user_id) AS DOUBLE) AS avg_ltv_per_user
FROM user_cohorts c
INNER JOIN monthly_revenue r ON c.user_id = r.user_id
GROUP BY 1, 2;