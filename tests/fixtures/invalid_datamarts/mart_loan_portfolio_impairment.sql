SELECT
    portfolio_id,
    stage_id,
    SUM(principal_outstanding) AS total_principal,
    SUM(interest_outstanding) AS total_interest,
    AVG(pd_score) AS weighted_pd,
    AVG(lgd_score) AS weighted_lgd,
    SUM(principal_outstanding * pd_score * lgd_score) AS expected_credit_loss,
    TO_CHAR(reporting_date, 'YYYY-MM-DD') AS formatted_report_date
FROM lakehouse.accounting.loans
WHERE reporting_date = LAST_DAY(ADD_MONTHS(CURRENT_DATE, -1))
GROUP BY portfolio_id, stage_id, reporting_date;