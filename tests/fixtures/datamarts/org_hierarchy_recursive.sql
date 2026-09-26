WITH RECURSIVE org_hierarchy AS (
    -- Anchor
    SELECT
        employee_id,
        manager_id,
        employee_name,
        1 AS hierarchy_level,
        CAST(employee_name AS VARCHAR(1000)) AS path_trace
    FROM raw_hr.employees
    WHERE manager_id IS NULL

    UNION ALL

    -- Recursive step
    SELECT
        e.employee_id,
        e.manager_id,
        e.employee_name,
        h.hierarchy_level + 1,
        CAST(h.path_trace || ' -> ' || e.employee_name AS VARCHAR(1000))
    FROM raw_hr.employees e
    JOIN org_hierarchy h ON e.manager_id = h.employee_id
)
SELECT
    employee_id,
    manager_id,
    employee_name,
    hierarchy_level,
    path_trace
FROM org_hierarchy;