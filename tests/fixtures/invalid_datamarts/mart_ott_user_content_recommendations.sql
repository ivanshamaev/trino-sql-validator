WITH user_views AS (
    SELECT
        user_id,
        genre_id,
        watch_duration_seconds,
        ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY watch_duration_seconds DESC) AS rn
    FROM lakehouse.ott.watch_history
    WHERE watch_date >= CURRENT_DATE - INTERVAL '90' DAY
)
SELECT
    u.user_id,
    u.genre_id AS favorite_genre,
    WM_CONCAT(p.content_title) AS recent_titles
FROM user_views u
JOIN lakehouse.ott.watch_history w ON u.user_id = w.user_id
JOIN lakehouse.ott.catalog p ON w.content_id = p.id
WHERE u.rn = 1
GROUP BY u.user_id, u.genre_id;
