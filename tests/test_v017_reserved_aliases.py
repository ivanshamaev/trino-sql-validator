from __future__ import annotations

import pytest
from trino_sql_validator import AliasWarning, validate

RESERVED_KEYWORDS = [
    "ALTER",
    "AND",
    "AS",
    "AUTO",
    "BETWEEN",
    "BY",
    "CASE",
    "CAST",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CUBE",
    "CURRENT_CATALOG",
    "CURRENT_DATE",
    "CURRENT_PATH",
    "CURRENT_ROLE",
    "CURRENT_SCHEMA",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "DEALLOCATE",
    "DELETE",
    "DESCRIBE",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "ESCAPE",
    "EXCEPT",
    "EXISTS",
    "EXTRACT",
    "FALSE",
    "FOR",
    "FROM",
    "FULL",
    "GROUP",
    "GROUPING",
    "HAVING",
    "IN",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "JSON_ARRAY",
    "JSON_EXISTS",
    "JSON_OBJECT",
    "JSON_QUERY",
    "JSON_TABLE",
    "JSON_VALUE",
    "LEFT",
    "LIKE",
    "LISTAGG",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "NATURAL",
    "NORMALIZE",
    "NOT",
    "NULL",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVERLAPS",
    "PREPARE",
    "RECURSIVE",
    "RIGHT",
    "ROLLUP",
    "SELECT",
    "SKIP",
    "TABLE",
    "THEN",
    "TRIM",
    "TRUE",
    "UESCAPE",
    "UNION",
    "UNNEST",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITH",
]

NON_RESERVED_KEYWORDS = [
    "ABSENT",
    "ADD",
    "ADMIN",
    "AFTER",
    "ALL",
    "ANALYZE",
    "ANY",
    "ARRAY",
    "ASC",
    "ASYMMETRIC",
    "AT",
    "AUTHORIZATION",
    "BEGIN",
    "BERNOULLI",
    "BOTH",
    "BRANCH",
    "BRANCHES",
    "CALL",
    "CALLED",
    "CASCADE",
    "CATALOG",
    "CATALOGS",
    "COLUMN",
    "COLUMNS",
    "COMMENT",
    "COMMIT",
    "COMMITTED",
    "CONDITIONAL",
    "COPARTITION",
    "CORRESPONDING",
    "COUNT",
    "CURRENT",
    "DATA",
    "DATE",
    "DAY",
    "DECLARE",
    "DEFAULT",
    "DEFINE",
    "DEFINER",
    "DENY",
    "DESC",
    "DESCRIPTOR",
    "DETERMINISTIC",
    "DISTRIBUTED",
    "DO",
    "DOUBLE",
    "ELSEIF",
    "EMPTY",
    "ENCODING",
    "ERROR",
    "EXCLUDING",
    "EXECUTE",
    "EXPLAIN",
    "FAIL",
    "FAST",
    "FETCH",
    "FILTER",
    "FINAL",
    "FIRST",
    "FOLLOWING",
    "FORMAT",
    "FORWARD",
    "FUNCTION",
    "FUNCTIONS",
    "GRACE",
    "GRANT",
    "GRANTED",
    "GRANTS",
    "GRAPHVIZ",
    "GROUPS",
    "HOUR",
    "IF",
    "IGNORE",
    "IMMEDIATE",
    "INCLUDING",
    "INITIAL",
    "INLINE",
    "INPUT",
    "INTERVAL",
    "INVOKER",
    "IO",
    "ITERATE",
    "ISOLATION",
    "JSON",
    "KEEP",
    "KEY",
    "KEYS",
    "LANGUAGE",
    "LAST",
    "LATERAL",
    "LEADING",
    "LEAVE",
    "LEVEL",
    "LIMIT",
    "LOCAL",
    "LOGICAL",
    "LOOP",
    "MAP",
    "MATCH",
    "MATCHED",
    "MATCHES",
    "MATCH_RECOGNIZE",
    "MATERIALIZED",
    "MEASURES",
    "MERGE",
    "MINUTE",
    "MONTH",
    "NEAREST",
    "NESTED",
    "NEXT",
    "NFC",
    "NFD",
    "NFKC",
    "NFKD",
    "NO",
    "NONE",
    "NULLIF",
    "NULLS",
    "OBJECT",
    "OF",
    "OFFSET",
    "OMIT",
    "ONE",
    "ONLY",
    "OPTION",
    "ORDINALITY",
    "OUTPUT",
    "OVER",
    "OVERFLOW",
    "OVERLAY",
    "PARTIAL",
    "PARTITION",
    "PARTITIONS",
    "PASSING",
    "PAST",
    "PATH",
    "PATTERN",
    "PER",
    "PERIOD",
    "PERMUTE",
    "PIVOT",
    "PLACING",
    "PLAN",
    "POSITION",
    "PRECEDING",
    "PRECISION",
    "PRIVILEGES",
    "PROPERTIES",
    "PRUNE",
    "QUOTES",
    "RANGE",
    "READ",
    "REFRESH",
    "RENAME",
    "REPEAT",
    "REPEATABLE",
    "REPLACE",
    "RESET",
    "RESPECT",
    "RESTRICT",
    "RETURN",
    "RETURNING",
    "RETURNS",
    "REVOKE",
    "ROLE",
    "ROLES",
    "ROLLBACK",
    "ROW",
    "ROWS",
    "RUNNING",
    "SCALAR",
    "SCHEMA",
    "SCHEMAS",
    "SECOND",
    "SECURITY",
    "SEEK",
    "SERIALIZABLE",
    "SESSION",
    "SET",
    "SETS",
    "SHOW",
    "SIMPLE",
    "SOME",
    "STALE",
    "START",
    "STATS",
    "SUBSET",
    "SUBSTRING",
    "SYMMETRIC",
    "SYSTEM",
    "TABLES",
    "TABLESAMPLE",
    "TEXT",
    "TEXT_STRING",
    "TIES",
    "TIME",
    "TIMESTAMP",
    "TO",
    "TRAILING",
    "TRANSACTION",
    "TRUNCATE",
    "TRY_CAST",
    "TYPE",
    "UNBOUNDED",
    "UNCOMMITTED",
    "UNCONDITIONAL",
    "UNIQUE",
    "UNKNOWN",
    "UNMATCHED",
    "UNTIL",
    "UPDATE",
    "USE",
    "USER",
    "UTF16",
    "UTF32",
    "UTF8",
    "VALIDATE",
    "VALUE",
    "VERBOSE",
    "VERSION",
    "VIEW",
    "WHILE",
    "WINDOW",
    "WITHIN",
    "WITHOUT",
    "WORK",
    "WRAPPER",
    "WRITE",
    "YEAR",
    "ZONE",
]

ALIAS_TEMPLATES = [
    "SELECT 1 AS {}",
    "SELECT 1 {}",
    "SELECT * FROM orders AS {}",
    "SELECT * FROM orders {}",
    "SELECT * FROM (SELECT 1) AS {}",
    "SELECT * FROM (SELECT 1) {}",
]


def test_keyword_catalogs_have_the_expected_shape() -> None:
    assert len(RESERVED_KEYWORDS) == 83
    assert RESERVED_KEYWORDS == sorted(set(RESERVED_KEYWORDS))
    assert len(NON_RESERVED_KEYWORDS) == 230
    assert set(RESERVED_KEYWORDS).isdisjoint(NON_RESERVED_KEYWORDS)


@pytest.mark.parametrize("word", RESERVED_KEYWORDS)
@pytest.mark.parametrize("template", ALIAS_TEMPLATES)
def test_unquoted_reserved_alias_is_invalid(word: str, template: str) -> None:
    result = validate(template.format(word))

    assert result.valid is False
    assert result.statement_count == 0
    assert result.warnings == ()


@pytest.mark.parametrize("word", RESERVED_KEYWORDS)
@pytest.mark.parametrize("template", ALIAS_TEMPLATES)
def test_double_quoted_reserved_alias_is_valid(word: str, template: str) -> None:
    result = validate(template.format(f'"{word}"'))

    assert result.valid is True, result.error
    assert result.warnings == ()


@pytest.mark.parametrize("word", RESERVED_KEYWORDS)
@pytest.mark.parametrize(
    "template",
    [
        "WITH {} AS (SELECT 1) SELECT * FROM x",
        "WITH x({}) AS (SELECT 1) SELECT * FROM x",
        "SELECT * FROM orders AS x({})",
    ],
)
def test_reserved_alias_in_identifier_list_is_invalid(word: str, template: str) -> None:
    result = validate(template.format(word))

    assert result.valid is False
    assert result.warnings == ()


@pytest.mark.parametrize("word", RESERVED_KEYWORDS)
@pytest.mark.parametrize(
    "template",
    [
        "WITH {} AS (SELECT 1) SELECT * FROM x",
        "WITH x({}) AS (SELECT 1) SELECT * FROM x",
        "SELECT * FROM orders AS x({})",
    ],
)
def test_double_quoted_reserved_alias_in_identifier_list_is_valid(word: str, template: str) -> None:
    result = validate(template.format(f'"{word}"'))

    assert result.valid is True, result.error
    assert result.warnings == ()


@pytest.mark.parametrize("word", RESERVED_KEYWORDS)
def test_double_quoted_reserved_column_reference_is_valid(word: str) -> None:
    for sql in [f'SELECT "{word}" FROM t', f'SELECT t."{word}" FROM t']:
        result = validate(sql)

        assert result.valid is True, result.error
        assert result.warnings == ()


@pytest.mark.parametrize("word", NON_RESERVED_KEYWORDS)
@pytest.mark.parametrize("template", ALIAS_TEMPLATES)
def test_non_reserved_alias_is_valid(word: str, template: str) -> None:
    result = validate(template.format(word))

    assert result.valid is True, result.error


@pytest.mark.parametrize("word", ["WHERE", "where", "WhErE"])
def test_reserved_alias_error_is_located_and_case_insensitive(word: str) -> None:
    sql = f"SELECT 1 AS {word}"
    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert result.error.line == 1
    assert result.error.column == sql.rindex(word) + 1


def test_lowercase_quoted_reserved_alias_is_valid() -> None:
    result = validate('SELECT 1 AS "where"')

    assert result.valid is True, result.error
    assert result.warnings == ()


def test_reserved_alias_in_later_statement_keeps_source_location() -> None:
    result = validate("SELECT 1;\nSELECT 2 AS WHERE")

    assert result.valid is False
    assert result.statement_count == 0
    assert result.error is not None
    assert result.error.line == 2
    assert result.error.column == 13
    assert result.warnings == ()


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 AS 'group'",
        "SELECT value AS 'group'",
        "SELECT 1 'group'",
        "SELECT * FROM orders AS 'group'",
        "SELECT * FROM orders 'group'",
        "SELECT * FROM (SELECT 1) AS 'group'",
        "SELECT * FROM (SELECT 1) 'group'",
        "SELECT 'group', 1 'group'",
        "SELECT 'group' FROM orders 'group'",
    ],
)
def test_single_quoted_alias_is_invalid_and_located(sql: str) -> None:
    result = validate(sql)

    assert result.valid is False
    assert result.error is not None
    assert result.error.line == 1
    assert result.error.column == sql.rindex("'group'") + 1
    assert result.warnings == ()


def test_typed_literal_is_not_misclassified_as_a_single_quoted_alias() -> None:
    result = validate("SELECT bignum 'x'")

    assert result.valid is True, result.error
    assert result.unknown_types == ["bignum"]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT x LIMIT 5",
        "SELECT x OFFSET 5 ROWS",
        "SELECT x FETCH FIRST 5 ROWS ONLY",
        "SELECT x WINDOW w AS (PARTITION BY y)",
        "SELECT x FROM t WINDOW w AS (PARTITION BY y)",
        "SELECT * FROM t TABLESAMPLE SYSTEM (10)",
        "SELECT * FROM t PIVOT (sum(x) FOR y IN (1))",
        "SELECT * FROM t MATCH_RECOGNIZE (PATTERN (A) DEFINE A AS true)",
        "WITH final AS (SELECT 1) SELECT * FROM final",
    ],
)
def test_real_clause_is_not_consumed_as_an_alias(sql: str) -> None:
    result = validate(sql)

    assert result.valid is True, result.error


def test_tablesample_is_an_alias_after_a_complex_relation_without_a_method() -> None:
    result = validate("SELECT * FROM UNNEST(ARRAY[1]) WITH ORDINALITY TABLESAMPLE")

    assert result.valid is True, result.error
    assert result.warnings == ()


@pytest.mark.parametrize("alias", ["all", "over", "partition", "return", "at"])
@pytest.mark.parametrize(
    "template",
    [
        "SELECT * FROM orders AS {}",
        "SELECT * FROM orders {}",
        "SELECT * FROM (SELECT 1) AS {}",
        "SELECT * FROM (SELECT 1) {}",
    ],
)
def test_contextual_relation_alias_remains_valid_with_warning(alias: str, template: str) -> None:
    sql = template.format(alias)
    result = validate(sql)

    assert result.valid is True, result.error
    assert result.warnings == (AliasWarning(alias, line=1, column=sql.rindex(alias) + 1),)


def test_quoted_reserved_identifiers_are_valid_in_ddl_and_select() -> None:
    ctas = """
CREATE TABLE dwh_team."FROM" AS
SELECT
  1 AS "ALTER",
  2 AS "TABLE",
  3 AS "BETWEEN",
  4 AS "FROM"
"""
    select = """
SELECT "FROM"."ALTER"
FROM dwh_team."FROM" AS "FROM"
"""

    for sql in [ctas, select]:
        result = validate(sql)

        assert result.valid is True, result.error
        assert result.warnings == ()


def test_non_trino_alias_policy_is_unchanged() -> None:
    for dialect in ["generic", "hive"]:
        result = validate("SELECT 1 AS WHERE", dialect=dialect)

        assert result.valid is True, result.error
