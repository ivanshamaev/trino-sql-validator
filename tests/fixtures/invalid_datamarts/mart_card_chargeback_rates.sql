SELECT
    merchant_id,
    mcc_code,
    SUBSTR(pan_masked, 1, 6) AS bin_number,
    COUNT(transaction_id) AS total_tx_count,
    SUM(amount) AS total_tx_volume,
    COUNT_IF(is_chargeback = true) AS chargeback_count,
    NVL(SUM(CASE WHEN is_chargeback = true THEN amount END), 0) AS chargeback_amount,
    RATIO_TO_REPORT(SUM(amount)) OVER (PARTITION BY mcc_code) AS merchant_volume_share
FROM lakehouse.acquiring.transactions
WHERE tx_date >= DATE '2026-08-01'
GROUP BY merchant_id, mcc_code, SUBSTR(pan_masked, 1, 6);