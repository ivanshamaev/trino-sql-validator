SELECT
    partner_id,
    call_type,
    DATE_TRUNC('day', call_start_time) AS call_date,
    COUNT(cdr_id) AS total_calls,
    SUM(CEIL(duration_seconds / 60.0)) AS billed_minutes,
    SUM(cost_amount) AS total_cost_usd,
    CORR(duration_seconds, cost_amount) AS duration_cost_correlation,
    TRUNC_NUMBER(SUM(cost_amount), 2) AS rounded_cost
FROM lakehouse.telecom.cdr_records
WHERE call_start_time >= TIMESTAMP '2026-09-01 00:00:00'
GROUP BY partner_id, call_type, DATE_TRUNC('day', call_start_time);
