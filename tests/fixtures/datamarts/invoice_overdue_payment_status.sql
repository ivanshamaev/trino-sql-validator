SELECT
    customer_id,
    invoice_id,
    due_date,
    payment_date,
    invoice_amount,
    COALESCE(date_diff('day', due_date, payment_date), date_diff('day', due_date, CURRENT_DATE)) AS overdue_days,
    CASE
        WHEN payment_date IS NULL AND CURRENT_DATE > due_date THEN 'UNPAID_OVERDUE'
        WHEN payment_date > due_date THEN 'PAID_LATE'
        ELSE 'ON_TIME'
    END AS payment_status,
    SUM(invoice_amount) OVER (
        PARTITION BY customer_id
        ORDER BY due_date
        ROWS BETWEEN 3 PRECEDING AND CURRENT ROW
    ) AS rolling_4_invoices_sum
FROM raw_fintech.invoices;