WITH ranked_touchpoints AS (
    SELECT
        user_id,
        campaign_id,
        touchpoint_timestamp,
        ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY touchpoint_timestamp ASC) AS ascii_rank_first,
        ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY touchpoint_timestamp DESC) AS ascii_rank_last
    FROM raw_adtech.touchpoints
),
conversions AS (
    SELECT user_id, conversion_id, amount, conversion_timestamp
    FROM raw_adtech.conversions
)
SELECT
    c.conversion_id,
    c.user_id,
    c.amount,
    FIRST_VALUE(tp_first.campaign_id) OVER (PARTITION BY c.user_id) AS first_touch_campaign,
    FIRST_VALUE(tp_last.campaign_id) OVER (PARTITION BY c.user_id) AS last_touch_campaign
FROM conversions c
LEFT JOIN ranked_touchpoints tp_first
       ON c.user_id = tp_first.user_id AND tp_first.ascii_rank_first = 1
LEFT JOIN ranked_touchpoints tp_last
       ON c.user_id = tp_last.user_id AND tp_last.ascii_rank_last = 1;