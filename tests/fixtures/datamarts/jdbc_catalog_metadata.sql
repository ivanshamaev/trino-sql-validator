SELECT
    t.table_cat AS catalog_name,
    t.table_schem AS schema_name,
    t.table_name,
    c.column_name,
    c.type_name AS data_type,
    c.column_size,
    CASE
        WHEN c.is_nullable = 'YES' THEN TRUE
        ELSE FALSE
    END AS is_nullable
FROM system.jdbc.tables t
JOIN system.jdbc.columns c
  ON t.table_cat = c.table_cat
 AND t.table_schem = c.table_schem
 AND t.table_name = c.table_name
WHERE t.table_schem NOT IN ('information_schema', 'sys')
ORDER BY catalog_name, schema_name, table_name, c.ordinal_position;