WITH RECURSIVE hierarchy(id, path, meta_source_system) AS (
    SELECT
        root.child AS id,
        CAST(ARRAY[] AS ARRAY(VARCHAR)) AS path,
        root.meta_source_system
    FROM dds_tmp.tmp_client_posting_postings_parent_child_recursive_base AS root
    UNION ALL
    SELECT
        full_row.child AS id,
        CAST((hierarchy.path || ARRAY[full_row.child]) AS ARRAY(VARCHAR)) AS path,
        full_row.meta_source_system
    FROM dds_tmp.tmp_client_posting_postings_parent_child_recursive_full AS full_row
    JOIN hierarchy
        ON hierarchy.id = full_row.parent
)
SELECT
    id,
    path,
    CARDINALITY(path) - 1 AS level,
    meta_source_system
FROM hierarchy;

SELECT
    client_order_id,
    payment_type_id
FROM (
    SELECT
        order_row.client_order_id,
        order_row.payment_type_id,
        ROW_NUMBER() OVER (
            PARTITION BY order_row.client_order_id
            ORDER BY order_row.start_time DESC
        ) AS row_number
    FROM dds_data.tie_client_order_payment_type_his AS order_row
    INNER JOIN dds_tmp.tmp_client_order_id_2 AS order_ids
        ON order_ids.client_order_id = order_row.client_order_id
) AS order_row
WHERE row_number = 1;

SELECT
    item_availability_change_id,
    CAST(SPLIT(reason_source_key, ',') AS ARRAY(BIGINT)) AS reason_source_key,
    CAST(SPLIT(previous_reason_source_key, ',') AS ARRAY(BIGINT)) AS previous_reason_source_key
FROM dds_tmp.tmp_deduplicated_iacr AS item_change
FULL OUTER JOIN dds_tmp.tmp_deduplicated_iacrp AS item_change_previous
    ON item_change_previous.item_availability_change_id = item_change.item_availability_change_id;

SELECT
    id,
    path,
    level
FROM (
    WITH RECURSIVE hierarchy(id, path) AS (
        SELECT
            root.child AS id,
            CAST(ARRAY[root.child] AS ARRAY(BIGINT)) AS path
        FROM dds_tmp.tmp_vw_base AS root
        UNION ALL
        SELECT
            full_row.child AS id,
            CAST((hierarchy.path || ARRAY[full_row.child]) AS ARRAY(BIGINT)) AS path
        FROM dds_tmp.tmp_vw_full AS full_row
        JOIN hierarchy
            ON hierarchy.id = full_row.parent
    )
    SELECT
        *,
        CARDINALITY(path) - 1 AS level
    FROM hierarchy
) AS hierarchy_result;
