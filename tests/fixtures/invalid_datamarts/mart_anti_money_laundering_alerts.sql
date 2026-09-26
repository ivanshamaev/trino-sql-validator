WITH rapid_transfers AS (
    SELECT
        sender_account_id,
        receiver_account_id,
        amount,
        transaction_time,
        LAG(transaction_time) OVER (PARTITION BY sender_account_id ORDER BY transaction_time) AS prev_time,
        RANGE_SUM(amount) OVER (PARTITION BY sender_account_id ORDER BY transaction_time RANGE BETWEEN INTERVAL '1' HOUR PRECEDING AND CURRENT ROW) AS rolling_1h_amount
    FROM lakehouse.core.wire_transfers
    WHERE transaction_date = CURRENT_DATE
)
SELECT
    sender_account_id,
    COUNT(1) AS transfer_count,
    MAX(rolling_1h_amount) AS peak_1h_volume,
    MIN(transaction_time) AS alert_window_start,
    MAX(transaction_time) AS alert_window_end
FROM rapid_transfers
WHERE rolling_1h_amount > 1000000
GROUP BY sender_account_id
HAVING COUNT(1) >= 5;
