WITH daily_account_transactions AS (
    SELECT
        account_id,
        CAST(transaction_time AS DATE) AS tx_date,
        SUM(CASE WHEN transaction_type = 'CREDIT' THEN amount ELSE -amount END) AS daily_net_change
    FROM raw_fintech.transactions
    GROUP BY 1, 2
),
running_balances AS (
    SELECT
        account_id,
        tx_date,
        daily_net_change,
        SUM(daily_net_change) OVER (
            PARTITION BY account_id
            ORDER BY tx_date
            ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
        ) AS current_balance,
        AVG(daily_net_change) OVER (
            PARTITION BY account_id
            ORDER BY tx_date
            ROWS BETWEEN 7 PRECEDING AND 1 PRECEDING
        ) AS avg_7d_change,
        STDDEV(daily_net_change) OVER (
            PARTITION BY account_id
            ORDER BY tx_date
            ROWS BETWEEN 7 PRECEDING AND 1 PRECEDING
        ) AS stddev_7d_change
    FROM daily_account_transactions
)
SELECT
    account_id,
    tx_date,
    daily_net_change,
    current_balance,
    avg_7d_change,
    CASE
        WHEN ABS(daily_net_change - avg_7d_change) > (2 * COALESCE(stddev_7d_change, 0))
        THEN TRUE ELSE FALSE
    END AS is_anomaly
FROM running_balances;