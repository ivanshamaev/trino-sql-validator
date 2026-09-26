WITH payment_history AS (
    SELECT
        contract_id,
        due_date,
        payment_date,
        amount_due,
        amount_paid,
        CASE WHEN payment_date IS NULL OR payment_date > due_date THEN 1 ELSE 0 END AS is_late,
        COALESCE(DATE_DIFF('day', due_date, payment_date), DATE_DIFF('day', due_date, CURRENT_DATE)) AS delay_days
    FROM lakehouse.credit.schedules
)
SELECT
    contract_id,
    COUNT(1) AS total_schedules,
    SUM(is_late) AS late_payment_count,
    MAX(delay_days) AS max_delay_days,
    AVG(delay_days) AS avg_delay_days,
    TOP_K(delay_days, 3) AS top_3_delays,
    STDDEV_SAMP(delay_days) AS delay_stddev
FROM payment_history
GROUP BY contract_id;