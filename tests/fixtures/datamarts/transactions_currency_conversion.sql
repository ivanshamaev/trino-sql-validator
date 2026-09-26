SELECT
    t.transaction_id,
    t.account_id,
    t.amount AS original_amount,
    t.currency_code AS original_currency,
    r.exchange_rate,
    t.amount * COALESCE(r.exchange_rate, 1.0) AS amount_usd
FROM raw_fintech.transactions t
LEFT JOIN raw_fintech.exchange_rates r
       ON t.currency_code = r.from_currency
      AND r.to_currency = 'USD'
      AND r.rate_date = (
          SELECT MAX(sub_r.rate_date)
          FROM raw_fintech.exchange_rates sub_r
          WHERE sub_r.from_currency = t.currency_code
            AND sub_r.to_currency = 'USD'
            AND sub_r.rate_date <= CAST(t.transaction_time AS DATE)
      )
WHERE t.transaction_time >= TIMESTAMP '2025-01-01 00:00:00';