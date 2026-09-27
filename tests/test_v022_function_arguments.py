from __future__ import annotations

from pathlib import Path

import pytest
from trino_sql_validator import (
    FunctionArgumentWarning,
    analyze_statements,
    validate,
    validate_file,
)

USER_REPRODUCTION = (
    "select id, date, sum() over(partition by id order by date) rn\n"
    "from schema.table_name\n"
    "where rn=1\n"
    ";"
)

DOCUMENTED_AGGREGATE_ARITIES = {
    "any_value": (1,),
    "approx_distinct": (1, 2),
    "approx_most_frequent": (3,),
    "approx_percentile": (2, 3),
    "approx_set": (1,),
    "arbitrary": (1,),
    "array_agg": (1,),
    "avg": (1,),
    "bitwise_and_agg": (1,),
    "bitwise_or_agg": (1,),
    "bitwise_xor_agg": (1,),
    "bool_and": (1,),
    "bool_or": (1,),
    "checksum": (1,),
    "corr": (2,),
    "count": (0, 1),
    "count_if": (1,),
    "covar_pop": (2,),
    "covar_samp": (2,),
    "every": (1,),
    "geometric_mean": (1,),
    "histogram": (1,),
    "kurtosis": (1,),
    "map_agg": (2,),
    "map_union": (1,),
    "max": (1, 2),
    "max_by": (2, 3),
    "merge": (1,),
    "min": (1, 2),
    "min_by": (2, 3),
    "multimap_agg": (2,),
    "numeric_histogram": (2, 3),
    "qdigest_agg": (1, 2, 3),
    "reduce_agg": (4,),
    "regr_intercept": (2,),
    "regr_slope": (2,),
    "skewness": (1,),
    "stddev": (1,),
    "stddev_pop": (1,),
    "stddev_samp": (1,),
    "sum": (1,),
    "tdigest_agg": (1, 2),
    "var_pop": (1,),
    "var_samp": (1,),
    "variance": (1,),
}


def test_sum_without_argument_from_user_reproduction_warns() -> None:
    result = validate(USER_REPRODUCTION, function_arguments="warn")

    assert result.valid is True
    assert result.statement_count == 1
    assert result.error is None
    assert result.warnings == (
        FunctionArgumentWarning(
            "sum", actual_count=0, expected_counts=(1,), line=1, column=18
        ),
    )
    assert result.function_argument_warnings == list(result.warnings)


def test_sum_without_argument_can_be_a_strict_error() -> None:
    result = validate(USER_REPRODUCTION, function_arguments="error")

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.message == (
        "function argument error: Trino built-in function 'sum' expects 1 argument; got 0"
    )
    assert (result.error.line, result.error.column) == (1, 18)
    assert result.warnings == (
        FunctionArgumentWarning("sum", 0, (1,), line=1, column=18),
    )


@pytest.mark.parametrize(
    ("name", "expected_counts"),
    [
        (name, expected_counts)
        for name, expected_counts in DOCUMENTED_AGGREGATE_ARITIES.items()
        if 0 not in expected_counts
    ],
)
def test_documented_aggregate_rejects_zero_arguments(
    name: str, expected_counts: tuple[int, ...]
) -> None:
    result = validate(f"SELECT {name}()")

    assert result.valid is False
    assert result.function_argument_warnings == [
        FunctionArgumentWarning(
            name,
            actual_count=0,
            expected_counts=expected_counts,
            line=1,
            column=8,
        )
    ]


@pytest.mark.parametrize(
    ("name", "argument_count"),
    [
        (name, argument_count)
        for name, expected_counts in DOCUMENTED_AGGREGATE_ARITIES.items()
        for argument_count in expected_counts
    ],
)
def test_documented_aggregate_accepts_supported_argument_counts(
    name: str, argument_count: int
) -> None:
    arguments = ", ".join(str(index) for index in range(1, argument_count + 1))
    result = validate(f"SELECT {name}({arguments})")

    assert result.valid is True, result.error
    assert result.function_argument_warnings == []


@pytest.mark.parametrize(
    ("name", "expected_counts"), DOCUMENTED_AGGREGATE_ARITIES.items()
)
def test_documented_aggregate_rejects_too_many_arguments(
    name: str, expected_counts: tuple[int, ...]
) -> None:
    argument_count = max(expected_counts) + 1
    arguments = ", ".join(str(index) for index in range(1, argument_count + 1))
    result = validate(f"SELECT {name}({arguments})")

    assert result.valid is False
    assert result.function_argument_warnings == [
        FunctionArgumentWarning(
            name,
            actual_count=argument_count,
            expected_counts=expected_counts,
            line=1,
            column=8,
        )
    ]


def test_function_argument_check_can_be_disabled() -> None:
    result = validate(USER_REPRODUCTION, function_arguments="off")

    assert result.valid is True
    assert result.warnings == ()


@pytest.mark.parametrize(
    ("sql", "actual_count"),
    [
        ("SELECT sum()", 0),
        ("SELECT sum(1, 2)", 2),
        ("SELECT sum() OVER ()", 0),
        ("WITH c AS (SELECT sum()) SELECT * FROM c", 0),
        ("SELECT (SELECT sum())", 0),
        ("SELECT sum(sum())", 0),
        ("SELECT ROW(sum(), 1).* AS (total, marker)", 0),
    ],
)
def test_sum_argument_counts_are_checked_in_nested_queries(
    sql: str, actual_count: int
) -> None:
    result = validate(sql)

    assert result.valid is False
    assert len(result.function_argument_warnings) == 1
    assert result.function_argument_warnings[0].actual_count == actual_count


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT sum(x) FROM t",
        "SELECT sum(DISTINCT x) FROM t",
        "SELECT sum(coalesce(x, 0)) FROM t",
        "SELECT sum(x) FILTER (WHERE x > 0) FROM t",
        "SELECT sum(x) OVER (PARTITION BY id ORDER BY date) FROM t",
        "SELECT count(), count(*), row_number() OVER (), rank() OVER ()",
        "SELECT listagg(x) WITHIN GROUP (ORDER BY x)",
        "SELECT sum(*) FROM t",
        "SELECT t.sum() FROM t",
        "SELECT bigint::sum()",
        'SELECT "sum"()',
        "WITH FUNCTION sum() RETURNS BIGINT RETURN 1 SELECT sum()",
    ],
)
def test_argument_check_does_not_overreach(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.function_argument_warnings == []


@pytest.mark.parametrize("dialect", ["generic", "hive"])
def test_function_argument_check_is_trino_only(dialect: str) -> None:
    result = validate("SELECT sum()", dialect=dialect, function_arguments="error")

    assert result.valid is True
    assert result.warnings == ()


def test_invalid_function_arguments_mode_raises() -> None:
    with pytest.raises(ValueError, match="unknown function_arguments mode"):
        validate("SELECT 1", function_arguments="invalid")  # type: ignore[arg-type]


def test_statement_analysis_reports_strict_error_statement() -> None:
    analysis = analyze_statements(
        "SELECT 1; SELECT sum(); SELECT 3", function_arguments="error"
    )

    assert analysis.validation.valid is False
    assert analysis.error_statement_index == 1
    assert len(analysis.statements) == 3


def test_validate_file_passes_function_argument_mode(tmp_path: Path) -> None:
    path = tmp_path / "sum.sql"
    path.write_text("SELECT sum()", encoding="utf-8")

    assert validate_file(path).function_argument_warnings
    assert validate_file(path, function_arguments="error").valid is False
    assert validate_file(path, function_arguments="off").warnings == ()


def test_jinja_mask_disables_argument_count_diagnostics() -> None:
    sql = "SELECT sum({% set arguments = 'x' %})"

    result = validate(sql, function_arguments="error")

    assert result.valid is True
    assert result.function_argument_warnings == []
