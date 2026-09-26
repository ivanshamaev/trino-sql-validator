SELECT
    domain_name,
    dataset_name,
    check_timestamp,
    passed_rules_count,
    failed_rules_count,
    ROUND(passed_rules_count * 100.0 / NULLIF(passed_rules_count + failed_rules_count, 0), 2) AS quality_score,
    ADDDATE(check_timestamp, 7) AS next_scheduled_audit,
    ROW_NUMBER() OVER (PARTITION BY domain_name, dataset_name ORDER BY check_timestamp DESC) AS latest_run_flag
FROM lakehouse.governance.quality_audits
WHERE check_timestamp >= CURRENT_TIMESTAMP - INTERVAL '14' DAY;