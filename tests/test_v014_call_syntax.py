from __future__ import annotations

import pytest
from trino_sql_validator import validate


@pytest.mark.parametrize(
    "sql",
    [
        "CALL system.custom_proc()",
        "CALL catalog.schema.custom_proc(1, 'value')",
        "CALL system.custom_proc(schema_name => 'sales', enabled => true)",
        "CALL system.custom_proc(missing_fn(1))",
    ],
)
def test_call_accepts_only_trino_procedure_shape(sql: str) -> None:
    result = validate(sql)

    assert result.valid, result.error
    assert "custom_proc" not in result.unknown_functions


@pytest.mark.parametrize(
    "sql",
    [
        "CALL system.register_table",
        "CALL system.register_table(DISTINCT 1)",
        "CALL system.register_table(*)",
        "CALL system.register_table(1) FILTER (WHERE TRUE)",
        "CALL system.register_table(1) OVER ()",
        "CALL system.register_table(name := 'x')",
        "CALL system.register_table(,)",
        "CALL system.register_table(1,)",
    ],
)
def test_call_rejects_non_trino_function_decorations(sql: str) -> None:
    assert validate(sql).valid is False


def test_call_argument_expressions_still_produce_function_warnings() -> None:
    sql = "CALL system.custom_proc(value => missing_fn(1))"

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_functions == ["missing_fn"]
    assert result.warnings[0].column == sql.index("missing_fn") + 1


@pytest.mark.parametrize(
    "sql",
    [
        "ALTER TABLE t EXECUTE optimize",
        "ALTER TABLE t EXECUTE optimize()",
        "ALTER TABLE t EXECUTE optimize(file_size_threshold => '30MB')",
        "ALTER TABLE t EXECUTE optimize(file_size_threshold => '30MB') WHERE id > 0",
        "ALTER MATERIALIZED VIEW mv EXECUTE refresh",
    ],
)
def test_table_execute_accepts_documented_shapes(sql: str) -> None:
    result = validate(sql)

    assert result.valid, result.error


@pytest.mark.parametrize(
    "sql",
    [
        "ALTER TABLE t EXECUTE system.optimize()",
        "ALTER TABLE IF EXISTS t EXECUTE optimize",
        "ALTER TABLE t EXECUTE optimize(file_size_threshold => '30MB') trailing",
    ],
)
def test_table_execute_rejects_non_trino_shapes(sql: str) -> None:
    assert validate(sql).valid is False


def test_table_execute_argument_and_where_expressions_keep_warnings() -> None:
    sql = (
        "ALTER TABLE t EXECUTE optimize(value => missing_arg(1)) "
        "WHERE missing_where(id)"
    )

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_functions == ["missing_arg", "missing_where"]
