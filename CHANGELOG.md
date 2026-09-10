# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] - 2026-09-10

### Added

- Added regression coverage for the documented SQL fixture corpus, including
  Trino examples and Iceberg statements.

### Fixed

- Added support for Trino `IPADDRESS` typed literals, Iceberg version and named
  branch references, additional `VALUES` forms, and `ALTER TABLE ... EXECUTE
  ... WHERE` statements.
- Recognized documented date/time expressions, `grouping`, and SQL/JSON
  functions so they no longer produce unknown-function warnings.

## [0.8.0] - 2026-09-07

### Fixed

- Rejected empty Trino `FROM` relations such as `SELECT a FROM WHERE` while
  preserving quoted keyword identifiers.
- Added regression coverage for the reported parser ambiguity.

## [0.7.0] - 2026-09-07

### Fixed

- Added support for Trino `ARRAY(type)` cast syntax used by recursive query
  examples.
- Allowed `TOP` as a Trino table alias and qualified identifier.
- Added inline regression coverage for recursive CTEs, nested subqueries,
  array casts, joins, and window functions.

## [0.6.0] - 2026-09-07

### Fixed

- Corrected Trino string literal handling so a backslash is treated as
  ordinary string content, including `CAST('\\' AS VARCHAR)`.
- Added regression coverage for recursive CTEs at top level and inside nested
  `FROM (...)` queries.

## [0.5.0] - 2026-09-07

### Added

- Added automatic Jinja/dbt template masking for `validate()` and
  `validate_file()` via `jinja="auto"` or `jinja="mask"`.
- Added strict `jinja="reject"` mode for validating unrendered templates as
  raw SQL.
- Added local regression coverage for dbt/Jinja SQL fixtures.

## [0.4.0] - 2026-09-07

### Fixed

- Preserved `CREATE FUNCTION` return types and body expressions for function
  and type warnings.
- Rejected Trino statements with unterminated groups.
- Preserved warning columns when normalizing `PREPARE ... FROM` syntax.

## [0.3.0] - 2026-09-07

### Added

- Trino **data-type validation**: for `dialect="trino"`, table/view columns,
  `ALTER TABLE` column operations, function return types and `CAST` targets are
  checked against a generated catalog of the documented Trino types. Unknown
  types are reported as `TypeWarning` via `ValidationResult.warnings` / new
  `ValidationResult.unknown_types`. The warning tuple is now `(kind, name, line,
  column)` with `kind` in `{"function", "type"}`. `tools/extract_types.py`
  regenerates the embedded catalog (`src/types.rs`) from the Trino docs.
- `TrinoDialect` statement coverage for Trino-only syntax: `CREATE/DROP CATALOG`
  and `BRANCH`, `CREATE FUNCTION`, `ALTER TABLE ... SET PROPERTIES / EXECUTE /
  SET AUTHORIZATION`, `ALTER VIEW / MATERIALIZED VIEW`, `RESET SESSION`, `SET
  PATH`, `SHOW CREATE SCHEMA / MATERIALIZED VIEW`, `DESCRIBE INPUT/OUTPUT`,
  `REFRESH MATERIALIZED VIEW`, and role `GRANT`/`REVOKE` (incl. `WITH ADMIN
  OPTION`, `REVOKE ADMIN OPTION FOR`). `PREPARE name FROM ...` is normalized to
  sqlparser's `AS` form. See `plan/sql-coverage.md`.
- Stricter handling for `CREATE FUNCTION`, `DESCRIBE INPUT/OUTPUT` and
  `RESET SESSION` so truncated forms are rejected instead of falling through to
  sqlparser's permissive fallback.

## [0.2.0] - 2026-09-06

### Added

- Trino **function-name validation**: for `dialect="trino"`, `validate()` and
  `validate_file()` now walk the parsed AST and flag calls to functions that are
  not in the documented Trino catalog (e.g. a misspelled `round`).
- New `ValidationResult.warnings` (`FunctionWarning`) and convenience property
  `ValidationResult.unknown_functions`. Unknown functions are non-fatal warnings
  — `valid` stays `True` because the syntax is fine.
- `tools/extract_functions.py` — regenerates the embedded catalog
  (`src/functions.rs`, 459 canonical names) from the Trino docs; committed so
  builds stay offline and deterministic.

## [0.1.0] - 2026-09-06

Initial release.

### Added

- `validate(sql, dialect=...)` — validate a string containing one or more Trino SQL
  statements.
- `validate_file(path, dialect=...)` — validate a UTF-8 `.sql` file.
- `ValidationResult` / `Error` result objects: invalid SQL is a value, not an
  exception; errors report message + line/column.
- Dialects: `trino` (custom `TrinoDialect` refusing backquoted identifiers),
  `hive`, `generic`.
- Rust core via PyO3 + `sqlparser-rs`: ABI3 wheel for Python >= 3.10.
- CI (`.github/workflows/ci.yml`): fmt, clippy, Rust + Python tests, ruff, mypy.
- Release pipeline (`.github/workflows/release.yml`): wheels for Linux
  (x86_64/aarch64 manylinux), macOS (arm64/x86_64), Windows (x86_64) + sdist,
  published to PyPI via Trusted Publishing on tag `v*`.
