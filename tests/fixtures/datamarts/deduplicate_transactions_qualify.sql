WITH ranked_transactions AS (
    SELECT
        transaction_id,
        account_id,
        amount,
        transaction_time,
        source_system,
        ROW_NUMBER() OVER (
            PARTITION BY account_id, amount, transaction_time
            ORDER BY ingested_at DESC
        ) AS duplicate_rank
    FROM raw_fintech.ingested_transactions
)
SELECT
    transaction_id,
    account_id,
    amount,
    transaction_time,
    source_system
FROM ranked_transactions
WHERE duplicate_rank > 1;
