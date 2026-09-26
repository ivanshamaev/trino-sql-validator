WITH nested_data AS (
    SELECT
        department_id,
        CAST(ROW('Manager', 95000.00) AS ROW(role VARCHAR, salary DOUBLE)) AS emp_info
    FROM raw_hr.departments
)
SELECT
    department_id,
    emp_info.role AS role_title,
    emp_info.salary AS base_salary,
    map_agg(department_id, emp_info.salary) AS dept_salary_map
FROM nested_data
GROUP BY department_id, emp_info.role, emp_info.salary;