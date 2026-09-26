SELECT
    session_id,
    user_id,
    min_event_time AS session_start,
    MAX(CASE WHEN event_type = 'page_view' THEN event_time END) AS view_time,
    MAX(CASE WHEN event_type = 'add_to_cart' THEN event_time END) AS cart_time,
    MAX(CASE WHEN event_type = 'checkout_start' THEN event_time END) AS checkout_time,
    MAX(CASE WHEN event_type = 'payment_success' THEN event_time END) AS payment_time,
    date_diff('second',
        MIN(event_time),
        MAX(CASE WHEN event_type = 'payment_success' THEN event_time END)
    ) AS total_duration_seconds,
    CASE
        WHEN MAX(CASE WHEN event_type = 'payment_success' THEN 1 ELSE 0 END) = 1 THEN 'Converted'
        WHEN MAX(CASE WHEN event_type = 'checkout_start' THEN 1 ELSE 0 END) = 1 THEN 'Abandoned Checkout'
        WHEN MAX(CASE WHEN event_type = 'add_to_cart' THEN 1 ELSE 0 END) = 1 THEN 'Abandoned Cart'
        ELSE 'Bounced'
    END AS funnel_drop_stage
FROM (
    SELECT
        session_id,
        user_id,
        event_type,
        event_time,
        MIN(event_time) OVER (PARTITION BY session_id) AS min_event_time
    FROM raw_events.web_clicks
    WHERE event_time >= CURRENT_TIMESTAMP - INTERVAL '7' DAY
)
GROUP BY session_id, user_id, min_event_time;