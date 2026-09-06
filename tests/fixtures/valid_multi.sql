-- valid_multi.sql — three statements separated by semicolons
SELECT 1;
SELECT * FROM users WHERE age > 18 ORDER BY name DESC;
INSERT INTO audit_log (event, ts) VALUES ('login', CURRENT_TIMESTAMP);