SELECT count(*) AS n
FROM events
GROUP BY event_type
HAVING count(*) > 10
ORDER BY n DESC
LIMIT ALL;