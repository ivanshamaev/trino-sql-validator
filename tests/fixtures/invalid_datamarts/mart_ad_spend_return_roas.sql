WITH ad_spend AS (
    SELECT
        ad_date,
        campaign_id,
        SUM(cost) AS total_cost
    FROM lakehouse.marketing.ad_costs
    GROUP BY ad_date, campaign_id
),
ad_revenue AS (
    SELECT
        order_date,
        campaign_id,
        SUM(order_amount) AS total_revenue
    FROM lakehouse.sales.orders
    WHERE campaign_id IS NOT NULL
    GROUP BY order_date, campaign_id
)
SELECT
    COALESCE(s.ad_date, r.order_date) AS report_date,
    COALESCE(s.campaign_id, r.campaign_id) AS campaign_id,
    NVL(s.total_cost, 0) AS spend,
    NVL(r.total_revenue, 0) AS revenue,
    CASE WHEN NVL(s.total_cost, 0) > 0 THEN (NVL(r.total_revenue, 0) / s.total_cost) * 100 ELSE 0 END AS roas_pct
FROM ad_spend s
FULL OUTER JOIN ad_revenue r ON s.campaign_id = r.campaign_id AND s.ad_date = r.order_date;