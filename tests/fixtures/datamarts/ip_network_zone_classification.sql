SELECT
    client_ip,
    ip_subnet,
    CASE
        WHEN contains('10.0.0.0/8', CAST(client_ip AS IPADDRESS)) THEN 'Internal Corp'
        WHEN contains('192.168.0.0/16', CAST(client_ip AS IPADDRESS)) THEN 'Local Subnet'
        ELSE 'External Public'
    END AS network_zone,
    COUNT(*) AS total_requests
FROM (
    SELECT
        client_ip,
        '10.0.0.0/8' AS ip_subnet
    FROM raw_logs.firewall_logs
    WHERE try_cast(client_ip AS IPADDRESS) IS NOT NULL
)
GROUP BY 1, 2, 3;
