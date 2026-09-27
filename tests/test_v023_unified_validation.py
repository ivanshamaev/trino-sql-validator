from pathlib import Path

import pytest
from trino_sql_validator import (
    AliasWarning,
    FunctionArgumentWarning,
    FunctionWarning,
    TypeWarning,
    _native,
    analyze_statements,
    validate,
    validate_file,
)

USER_SCRIPT = (
    "select id, date, sum() over(partition by id order by date) rn\n"
    "from schema.table_name\n"
    "where rn=1\n;\n"
    "select id, date, min() over(partition by id order by date) rn\n"
    "from schema.table_name\n"
    "where rn=1\n;"
)


def test_default_rejects_user_script_and_reports_both_calls() -> None:
    result = validate(USER_SCRIPT)

    assert not result
    assert result.statement_count == 0
    assert result.error is not None
    assert "'sum' expects 1 argument; got 0" in result.error.message
    assert (result.error.line, result.error.column) == (1, 18)
    assert result.function_argument_warnings == [
        FunctionArgumentWarning("sum", 0, (1,), line=1, column=18),
        FunctionArgumentWarning("min", 0, (1, 2), line=5, column=18),
    ]
    assert tuple(result.function_argument_warnings) == result.warnings
    assert "warnings=2" in repr(result)
    assert result == validate(USER_SCRIPT, function_arguments="error")
    advisory = validate(USER_SCRIPT, function_arguments="warn")
    assert advisory.valid
    assert advisory.error is None
    assert advisory.statement_count == 2
    assert advisory.warnings == result.warnings


def test_arity_error_retains_all_four_diagnostic_kinds() -> None:
    sql = "SELECT my_missing_fn(), sum(), CAST(1 AS missing_type) AS over; SELECT min()"
    result = validate(sql)

    assert not result.valid
    assert result.error is not None
    assert (result.error.line, result.error.column) == (1, sql.index("sum") + 1)
    assert [type(warning) for warning in result.warnings] == [
        FunctionWarning,
        FunctionArgumentWarning,
        TypeWarning,
        AliasWarning,
        FunctionArgumentWarning,
    ]
    assert result.warnings == validate(sql, function_arguments="warn").warnings
    assert result.unknown_functions == ["my_missing_fn"]
    assert result.unknown_types == ["missing_type"]
    assert result.ambiguous_aliases == ["over"]


def test_catalog_and_alias_warnings_alone_remain_advisory() -> None:
    result = validate("SELECT my_missing_fn(), CAST(1 AS missing_type) AS over")

    assert result.valid
    assert result.error is None
    assert len(result.warnings) == 3
    assert not result.function_argument_warnings


def test_default_analysis_keeps_statements_and_first_arity_error_index() -> None:
    sql = "SELECT my_missing_fn();\n" + USER_SCRIPT
    analysis = analyze_statements(sql)

    assert not analysis
    assert analysis.validation == validate(sql)
    assert len(analysis.statements) == 3
    assert analysis.error_statement_index == 1
    assert len(analysis.validation.function_argument_warnings) == 2


@pytest.mark.parametrize("jinja", ["auto", "reject"])
def test_file_default_preserves_diagnostics(tmp_path: Path, jinja: str) -> None:
    path = tmp_path / "aggregates.sql"
    path.write_text(USER_SCRIPT, encoding="utf-8")

    result = validate_file(path, jinja=jinja)  # type: ignore[arg-type]

    assert not result.valid
    assert result == validate(USER_SCRIPT)


def test_native_defaults_match_public_api(tmp_path: Path) -> None:
    path = tmp_path / "aggregates.sql"
    path.write_text(USER_SCRIPT, encoding="utf-8")
    result = _native.validate(USER_SCRIPT)

    assert result[0] is False
    assert result[1] == 0
    assert result[3:5] == (1, 18)
    assert [tuple(w) for w in result[5]] == [
        ("function_arguments", "sum", 1, 18, 0, [1]),
        ("function_arguments", "min", 5, 18, 0, [1, 2]),
    ]
    assert result == _native.validate_file(str(path))
    analysis = _native.analyze_statements(USER_SCRIPT)
    assert result == analysis[0]
    assert len(analysis[1]) == 2
    assert analysis[2] == 0


@pytest.mark.parametrize("sql", ["SELECT sum(); SELECT FROM", "SELECT FROM; SELECT min()"])
def test_parser_errors_take_priority_and_have_no_partial_warnings(sql: str) -> None:
    result = validate(sql)

    assert not result.valid
    assert result.error is not None
    assert result.error.message.startswith("sql parser error:")
    assert result.warnings == ()


def test_repeated_aggregate_calls_keep_separate_positions() -> None:
    result = validate("SELECT sum(), sum(); SELECT sum()")

    assert not result.valid
    assert [(w.name, w.column) for w in result.function_argument_warnings] == [
        ("sum", 8),
        ("sum", 15),
        ("sum", 29),
    ]


def test_metadata_and_main_ast_diagnostics_are_deduplicated() -> None:
    sql = "SELECT ROW(sum(), min()).* AS (total, smallest)"
    result = validate(sql)

    assert not result.valid
    assert [(w.name, w.column) for w in result.function_argument_warnings] == [
        ("sum", 12),
        ("min", 19),
    ]


def test_inline_declaration_exemption_does_not_leak_to_next_statement() -> None:
    sql = "WITH FUNCTION sum() RETURNS BIGINT RETURN 1 SELECT sum(); SELECT sum()"
    result = validate(sql)

    assert not result.valid
    assert result.function_argument_warnings == [
        FunctionArgumentWarning("sum", 0, (1,), line=1, column=sql.rindex("sum") + 1)
    ]
