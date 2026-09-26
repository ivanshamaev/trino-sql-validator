SELECT
    request_id,
    user_ip,
    url_extract_path(request_url) AS endpoint_path,
    url_extract_parameter(request_url, 'utm_source') AS utm_source,
    json_extract_scalar(payload_json, '$.user.device.os') AS device_os,
    json_extract_scalar(payload_json, '$.checkout.total') AS payload_total,
    CAST(json_extract(payload_json, '$.items') AS ARRAY(MAP(VARCHAR, VARCHAR))) AS items_array
FROM raw_logs.http_requests
WHERE try_cast(json_extract_scalar(payload_json, '$.checkout.total') AS DOUBLE) > 100.0;