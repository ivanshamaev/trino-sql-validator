from __future__ import annotations

import pytest
from trino_sql_validator import (
    StatementAnalysis,
    StatementInfo,
    analyze_statements,
    validate,
)


def test_statement_analysis_is_opt_in_and_preserves_validation_result() -> None:
    sql = "SELECT missing_analysis(1); CREATE TABLE t(x BIGINT)"

    analysis = analyze_statements(sql)

    assert isinstance(analysis, StatementAnalysis)
    assert analysis.validation == validate(sql)
    assert analysis.validation.unknown_functions == ["missing_analysis"]
    assert analysis.error_statement_index is None
    assert [statement.kind for statement in analysis.statements] == ["query", "create_table"]
    assert analysis.statements[0] == StatementInfo(
        index=0,
        start_line=1,
        start_column=1,
        end_line=1,
        end_column=27,
        kind="query",
    )


@pytest.mark.parametrize(
    ("sql", "kind"),
    [
        ("SELECT 1", "query"),
        ("CREATE TABLE t(x BIGINT)", "create_table"),
        ("ALTER TABLE t RENAME TO u", "alter_table"),
        ("DROP TABLE t", "drop_table"),
        ("INSERT INTO t VALUES (1)", "insert"),
        ("UPDATE t SET x = 1", "update"),
        ("DELETE FROM t", "delete"),
        ("MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE", "merge"),
        ("CALL system.custom_proc()", "call"),
        ("SET SESSION query_max_memory = '1GB'", "set_session"),
        ("CREATE CATALOG memory USING memory", "create_catalog"),
        ("CREATE BRANCH audit IN TABLE t", "create_branch"),
    ],
)
def test_statement_kinds_come_from_original_source(sql: str, kind: str) -> None:
    analysis = analyze_statements(sql)

    assert analysis.validation.valid, analysis.validation.error
    assert len(analysis.statements) == 1
    assert analysis.statements[0].kind == kind


@pytest.mark.parametrize(
    ("sql", "kind", "inner_kind"),
    [
        ("EXPLAIN SELECT 1", "explain", "query"),
        ("EXPLAIN ALTER TABLE t SET PROPERTIES x = 1", "explain", "alter_table"),
        ("PREPARE p FROM SELECT 1", "prepare", "query"),
        (
            "PREPARE p FROM CREATE TABLE t(x BIGINT)",
            "prepare",
            "create_table",
        ),
    ],
)
def test_wrapper_and_inner_statement_kinds(sql: str, kind: str, inner_kind: str) -> None:
    statement = analyze_statements(sql).statements[0]

    assert statement.kind == kind
    assert statement.inner_kind == inner_kind


def test_routine_semicolons_do_not_create_phantom_statement_metadata() -> None:
    sql = (
        "CREATE FUNCTION f(x BIGINT) RETURNS BIGINT BEGIN "
        "DECLARE y BIGINT; IF x > 0 THEN RETURN x; END IF; RETURN y; END;\n"
        "SELECT 2"
    )

    analysis = analyze_statements(sql)

    assert analysis.validation.valid, analysis.validation.error
    assert [statement.kind for statement in analysis.statements] == ["create_function", "query"]
    assert analysis.statements[1].start_line == 2


def test_invalid_script_identifies_source_statement_without_partial_validation() -> None:
    sql = "SELECT 1;\nSELECT * FORM broken"

    analysis = analyze_statements(sql)

    assert analysis.validation.valid is False
    assert analysis.validation.statement_count == 0
    assert analysis.validation.warnings == ()
    assert analysis.error_statement_index == 1
    assert [statement.index for statement in analysis.statements] == [0, 1]


def test_statement_analysis_respects_existing_jinja_mode() -> None:
    sql = "SELECT {{ value }} AS rendered"

    assert analyze_statements(sql).validation.valid is True
    assert analyze_statements(sql, jinja="reject").validation.valid is False


def test_statement_analysis_rejects_unknown_dialect() -> None:
    with pytest.raises(ValueError, match="unknown dialect"):
        analyze_statements("SELECT 1", dialect="missing")
