WITH funnel_events AS (
    SELECT
        session_id,
        user_id,
        product_id,
        event_type,
        event_timestamp,
        LEAD(event_type, 1) OVER (PARTITION BY session_id, product_id ORDER BY event_timestamp) AS next_event
    FROM lakehouse.events.clickstream
    WHERE event_date = CURRENT_DATE - INTERVAL '1' DAY
)
SELECT
    p.category_id,
    COUNT(DISTINCT CASE WHEN e.event_type = 'view' THEN e.session_id END) AS view_sessions,
    COUNT(DISTINCT CASE WHEN e.event_type = 'add_to_cart' THEN e.session_id END) AS cart_sessions,
    COUNT(DISTINCT CASE WHEN e.event_type = 'checkout' THEN e.session_id END) AS checkout_sessions,
    COUNT(DISTINCT CASE WHEN e.event_type = 'purchase' THEN e.session_id END) AS purchase_sessions,
    ISNULL(COUNT(DISTINCT CASE WHEN e.event_type = 'purchase' THEN e.session_id END) * 100.0 /
           NULLIF(COUNT(DISTINCT CASE WHEN e.event_type = 'view' THEN e.session_id END), 0), 0) AS view_to_buy_rate
FROM funnel_events e
JOIN lakehouse.products.items p ON e.product_id = p.id
GROUP BY p.category_id;