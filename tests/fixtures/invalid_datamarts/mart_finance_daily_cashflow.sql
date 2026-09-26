SELECT
    account_id,
    transaction_date,
    SUM(CASE WHEN transaction_type = 'CREDIT' THEN amount ELSE 0 END) AS total_credit,
    SUM(CASE WHEN transaction_type = 'DEBIT' THEN amount ELSE 0 END) AS total_debit,
    SUM(CASE WHEN transaction_type = 'CREDIT' THEN amount ELSE -amount END) AS daily_net_change,
    SUM(SUM(CASE WHEN transaction_type = 'CREDIT' THEN amount ELSE -amount END))
        OVER (PARTITION BY account_id ORDER BY transaction_date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS running_balance,
    ZEROIFNULL(AVG(amount)) AS avg_transaction_size
FROM lakehouse.finance.transactions
WHERE status = 'POSTED'
GROUP BY account_id, transaction_date;