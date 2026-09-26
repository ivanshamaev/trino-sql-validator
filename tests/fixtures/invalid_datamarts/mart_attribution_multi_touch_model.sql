WITH user_touchpoints AS (
    SELECT
        user_id,
        campaign_id,
        channel_name,
        touch_timestamp,
        FIRST_VALUE(campaign_id) OVER (PARTITION BY user_id ORDER BY touch_timestamp ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS first_campaign,
        LAST_VALUE(campaign_id) OVER (PARTITION BY user_id ORDER BY touch_timestamp ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS last_campaign
    FROM lakehouse.marketing.touchpoints
)
SELECT
    campaign_id,
    COUNT(DISTINCT user_id) AS total_touched_users,
    COUNT(DISTINCT CASE WHEN campaign_id = first_campaign THEN user_id END) AS first_touch_conversions,
    COUNT(DISTINCT CASE WHEN campaign_id = last_campaign THEN user_id END) AS last_touch_conversions,
    ISDATE(MAX(touch_timestamp)) AS is_valid_date_check
FROM user_touchpoints
GROUP BY campaign_id;