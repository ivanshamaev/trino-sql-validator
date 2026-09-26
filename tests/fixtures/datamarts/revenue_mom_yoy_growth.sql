WITH monthly_metrics AS (
    SELECT
        date_trunc('month', order_date) AS m_date,
        SUM(revenue) AS current_revenue
    FROM analytics.mart_daily_sales_summary
    GROUP BY 1
)
SELECT
    m_date,
    current_revenue,
    LAG(current_revenue, 1) OVER (ORDER BY m_date) AS prev_month_revenue,
    LAG(current_revenue, 12) OVER (ORDER BY m_date) AS prev_year_revenue,
    (current_revenue - LAG(current_revenue, 1) OVER (ORDER BY m_date))
        / NULLIF(LAG(current_revenue, 1) OVER (ORDER BY m_date), 0) * 100.0 AS mom_growth_pct,
    (current_revenue - LAG(current_revenue, 12) OVER (ORDER BY m_date))
        / NULLIF(LAG(current_revenue, 12) OVER (ORDER BY m_date), 0) * 100.0 AS yoy_growth_pct
FROM monthly_metrics;