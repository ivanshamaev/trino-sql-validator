# Trino SQL-statement coverage & data-type validation

Status:
- SQL-doc coverage: implemented; 85/85 statements from `docs/src/main/sphinx/sql`
  (`/tmp/opencode/gap_analysis.py` corpus) parse with `dialect="trino"`.
- Real-world corpus: 78/81 statements from
  `trinodb/reports/sql/*`, trino-dbt-demo `customers.sql`,
  trino-the-definitive-guide `tpch/*` + `iris-data-set/*` parse (see below for the 3).
- Data-type validation: implemented (name-existence warnings), see
  `src/types.rs` + `functions-validation.md` for the shared design.

## Trino-only statements handled by `TrinoDialect`

`src/dialects/trino_statements.rs` adds token-shape recognizers for statements
sqlparser has no AST for; recognized statements return a placeholder so the
statement count still works. Dispatch happens from `Dialect::parse_statement`
via `try_parse_statement`, using `maybe_parse` for backtracking on every
recognizer, plus two deterministic branches:

- `CREATE [OR REPLACE] [TEMPORARY] FUNCTION name(...) RETURNS ... RETURN ...` is
  parsed strictly (no sqlparser fallback), so a missing `RETURNS`/`RETURN` is an
  error instead of a silent false-accept.
- `DESCRIBE INPUT/OUTPUT` and `RESET SESSION` are parsed strictly when their
  keyword follows `DESCRIBE`/`RESET`, so `DESCRIBE INPUT` (no name) and a bare
  `RESET SESSION` are rejected rather than read as a table called `input`.

Covered: RESET SESSION [AUTHORIZATION], SET PATH, SHOW CREATE SCHEMA /
MATERIALIZED VIEW, DESCRIBE INPUT/OUTPUT [(query) [WHERE]], REFRESH
MATERIALIZED VIEW, CREATE/DROP CATALOG and BRANCH, CREATE FUNCTION,
ALTER TABLE (SET PROPERTIES / EXECUTE / SET AUTHORIZATION), ALTER VIEW,
ALTER MATERIALIZED VIEW, ALTER BRANCH, GRANT/REVOKE roles (WITH ADMIN OPTION,
REVOKE ADMIN OPTION FOR).

Other subtle Trino syntax is normalized before parsing:
- `PREPARE name FROM <query>` → `PREPARE name AS <query>` (sqlparser only knows
  `AS`); done in `validate_sql_impl` with a regex, trino only.

## Data-type validation

`src/types.rs` is a generated catalog of the built-in Trino types (from
`docs/src/main/sphinx/language/types.md`; 36 names incl. aliases `INT`, base
names for `TIME(P)`, `TIMESTAMP WITH TIME ZONE`, ...). `find_unknown_types` in
`src/lib.rs` walks table/view columns, `ALTER TABLE` column operations, function
`RETURNS` types and every `CAST()`/`TRY_CAST()` target; unknown names are
reported as `TypeWarning`. The Rust→Python warning tuple is now
`(kind, name, line, column)` with `kind` in `{"function", "type"}`; warnings are
sorted by position.

## Known limitations (accepted, do not "fix" by loosening the dialect)

- sqlparser has no dialect hook for data-type parsing. The Trino compatibility
  layer therefore rewrites only recognized type-context tokens for nested
  `ROW`, `ARRAY`, and `MAP` definitions into sqlparser's supported structural
  forms. Original token spans are retained for warnings and errors, and
  `ROW(...)` value constructors are parsed without rewriting.
- Function/type presence is a name-existence check; arity, argument types,
  precision/scale and semantics are out of scope (syntax validator).
- dbt/Jinja templates are supported at the Python API boundary with the
  default `jinja="auto"` mode. Tags are masked without shifting line/column
  offsets, so expressions such as `{{ ref(...) }}` and `{{ var(...) }}` can be
  checked together with surrounding SQL without installing dbt. Use
  `jinja="reject"` for strict raw parsing, and validate dbt-rendered SQL when
  control-flow blocks, macros, or adapter-specific semantics determine the
  generated SQL shape.
