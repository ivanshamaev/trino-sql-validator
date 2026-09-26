SELECT
    raw_record_id,
    raw_numeric_string,
    try_cast(raw_numeric_string AS BIGINT) AS parsed_bigint,
    try(1000 / try_cast(raw_numeric_string AS BIGINT)) AS safe_division_result,
    COALESCE(try_cast(raw_date_string AS DATE), DATE '1970-01-01') AS safe_parsed_date
FROM raw_staging.corrupted_imports;