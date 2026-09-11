from __future__ import annotations

from pathlib import Path

import pytest
from trino_sql_validator import validate, validate_file

WRAPPERS = {
    "root": lambda query: query,
    "explain": lambda query: f"EXPLAIN {query}",
    "prepare": lambda query: f"PREPARE p FROM {query}",
    "ctas": lambda query: f"CREATE TABLE target AS {query}",
    "view": lambda query: f"CREATE VIEW target AS {query}",
    "insert": lambda query: f"INSERT INTO target {query}",
}

QUERY_FAMILIES = {
    "with_function": (
        "WITH FUNCTION f(x BIGINT) RETURNS BIGINT RETURN x "
        "WITH q AS (SELECT f(1) x) SELECT x FROM q"
    ),
    "corresponding": "SELECT 1 x UNION CORRESPONDING BY (x) SELECT 2 x",
    "pivot": "SELECT * FROM sales PIVOT (sum(amount) FOR month IN (1) GROUP BY region)",
    "nearest": "SELECT * FROM a CROSS JOIN NEAREST (FROM b MATCH b.ts <= a.ts)",
    "row_expansion": "SELECT ROW(1, 2).* AS (x, y)",
    "group_by_all": "SELECT region, sum(amount) FROM sales GROUP BY ALL region",
    "nested_row": "SELECT CAST(NULL AS ARRAY(ROW(x BIGINT, y ARRAY(VARCHAR))))",
    "json_table": "SELECT * FROM JSON_TABLE('{}', '$' COLUMNS(x BIGINT PATH '$.x'))",
    "match_subset": (
        "SELECT * FROM t MATCH_RECOGNIZE (PATTERN (A B) SUBSET U = (A, B) DEFINE B AS x > PREV(x))"
    ),
}

COMPOSITION_CASES = [
    (family, wrapper, wrap(query))
    for family, query in QUERY_FAMILIES.items()
    for wrapper, wrap in WRAPPERS.items()
]
COMPOSITION_CASES += [
    (
        "with_session",
        wrapper,
        WRAPPERS[wrapper]("WITH SESSION query_max_memory = '1GB' SELECT 1"),
    )
    for wrapper in ("root", "explain", "prepare")
]


def test_composition_matrix_has_all_57_classified_cases() -> None:
    assert len(COMPOSITION_CASES) == 57
    assert len({(family, wrapper) for family, wrapper, _ in COMPOSITION_CASES}) == 57


@pytest.mark.parametrize(
    ("family", "wrapper", "sql"),
    COMPOSITION_CASES,
    ids=[f"{family}::{wrapper}" for family, wrapper, _ in COMPOSITION_CASES],
)
def test_query_features_compose_with_supported_wrappers(
    family: str, wrapper: str, sql: str
) -> None:
    result = validate(sql)

    assert result.valid, f"{family}::{wrapper}: {result.error}"
    assert result.statement_count == 1
    assert result.warnings == (), f"{family}::{wrapper}: {result.warnings}"


@pytest.mark.parametrize(
    "variant",
    [
        "SELECT missing_meta(1)",
        "  SELECT missing_meta(1)",
        "SELECT /* preserved */ missing_meta(1)",
        "select MISSING_META(1)",
        "SELECT\r\nmissing_meta(1)",
    ],
)
def test_whitespace_comments_case_and_crlf_preserve_warning_identity(variant: str) -> None:
    result = validate(variant)

    assert result.valid, result.error
    assert result.unknown_functions == ["missing_meta"]
    warning = result.warnings[0]
    prefix = variant[: variant.lower().index("missing_meta")]
    assert warning.line == prefix.count("\n") + 1
    assert warning.column == len(prefix.rsplit("\n", 1)[-1]) + 1


def test_statement_concatenation_preserves_inline_scope_and_warning_positions() -> None:
    first = "WITH FUNCTION scoped(x BIGINT) RETURNS BIGINT RETURN x SELECT scoped(1)"
    second = "SELECT scoped(2), missing_neighbor(3)"
    sql = f"{first};\n{second}"

    result = validate(sql)

    assert result.valid, result.error
    assert result.statement_count == 2
    assert result.unknown_functions == ["scoped", "missing_neighbor"]
    assert [warning.line for warning in result.warnings] == [2, 2]


def test_diagnostics_survive_deep_query_feature_interactions() -> None:
    sql = """
        WITH q AS (
            SELECT transform(ARRAY[1, 2], x -> missing_lambda(x)) AS arr_values
        )
        SELECT missing_window(value) OVER (),
               CAST(missing_cast(value) AS ROW(item bignum))
        FROM q
        CROSS JOIN UNNEST(arr_values) AS u(value)
        WHERE value IN (SELECT missing_subquery(1))
        UNION ALL
        SELECT missing_union(1), CAST(NULL AS ROW(item bignum))
    """

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_functions == [
        "missing_lambda",
        "missing_window",
        "missing_cast",
        "missing_subquery",
        "missing_union",
    ]
    assert result.unknown_types == ["bignum", "bignum"]


def test_json_table_and_match_recognize_diagnostics_compose() -> None:
    sql = """
        WITH source AS (
            SELECT jt.x
            FROM JSON_TABLE(
                '[{"x":1}]',
                'lax $[*]' COLUMNS(x BIGINT PATH 'lax $.x')
            ) AS jt
        )
        SELECT missing_measure(x) OVER ()
        FROM source MATCH_RECOGNIZE (
            MEASURES missing_pattern(LAST(A.x)) AS y
            PATTERN (A+)
            DEFINE A AS missing_define(A.x) > 0
        )
    """

    result = validate(sql)

    assert result.valid, result.error
    assert result.unknown_functions == [
        "missing_measure",
        "missing_pattern",
        "missing_define",
    ]


@pytest.mark.parametrize(
    ("sql", "fragment"),
    [
        ("SELECT 'unterminated", "Unterminated string literal"),
        ("SELECT * FROM", "Expected"),
        ("ALTER TABLE t ADD COLUMN", "Expected: identifier"),
    ],
)
def test_errors_refer_to_original_sql(sql: str, fragment: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert fragment.lower() in result.error.message.lower()
    if result.error.line is not None:
        assert result.error.line == 1
        assert result.error.column is not None
        assert result.error.column <= len(sql) + 1


def test_missing_relation_guard_reports_the_original_keyword_position() -> None:
    sql = "SELECT * FROM WHERE x = 1"

    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert result.error.line == 1
    assert result.error.column == sql.index("WHERE") + 1


@pytest.mark.parametrize("jinja", ["auto", "mask", "reject"])
def test_validate_and_validate_file_match_for_crlf_sql(tmp_path: Path, jinja: str) -> None:
    sql = "SELECT 1;\r\nSELECT missing_file(2)"
    path = tmp_path / "query.sql"
    path.write_text(sql, encoding="utf-8", newline="")

    assert validate_file(path, jinja=jinja) == validate(sql, jinja=jinja)


@pytest.mark.parametrize("jinja", ["auto", "mask", "reject"])
def test_validate_and_validate_file_match_for_legacy_jinja(tmp_path: Path, jinja: str) -> None:
    sql = "SELECT {{ dbt_value }} AS value\r\n"
    path = tmp_path / "model.sql"
    path.write_text(sql, encoding="utf-8", newline="")

    assert validate_file(path, jinja=jinja) == validate(sql, jinja=jinja)


def test_invalid_file_result_remains_atomic(tmp_path: Path) -> None:
    sql = "SELECT missing_before(1);\nSELECT * FORM broken"
    path = tmp_path / "invalid.sql"
    path.write_text(sql, encoding="utf-8")

    result = validate_file(path)

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()
