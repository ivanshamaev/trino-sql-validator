# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.22.0] - 2026-09-27

### Added

- Added source-located argument-count diagnostics for 45 documented,
  unqualified built-in aggregates. The registry includes overloaded arities
  such as `min/max: {1, 2}`, `min_by/max_by: {2, 3}`, approximate aggregates,
  statistical aggregates, map aggregates, and digest aggregates.
- Added `function_arguments="warn" | "error" | "off"` to `validate()`,
  `validate_file()`, and `analyze_statements()`. Advisory mode is the default;
  strict mode returns an invalid result and off mode preserves syntax-only
  behavior.
- Added `FunctionArgumentWarning` with the actual and expected argument counts.

### Boundaries

- Preserved valid zero-argument functions, qualified calls, quoted names,
  wildcard/named forms, and query-scoped inline functions. General overload,
  type, and column resolution remain outside the focused rule.
- Disabled argument-count inference when Jinja masking changes the input;
  rendered SQL remains authoritative.

### Verification

- Added focused Rust and public Python regressions for the reported query,
  all three modes, nested queries, CTEs, compatibility metadata, inline
  functions, dialect isolation, source positions, statement indexes, and
  Jinja masking.
- Verified 5,188 pytest cases, 90 Rust tests, the no-PyO3 core and fuzz-target
  builds, both generated catalogs, and the pinned Trino 483 parser audit with
  no baseline regressions.

## [0.21.0] - 2026-09-27

### Fixed

- Rejected PostgreSQL-style `expression::type` casts in the Trino dialect,
  including dereferences, literals, parenthesized expressions, arrays, and
  chained forms that the delegated Generic grammar previously accepted.
- Preserved Trino static method calls with the exact
  `qualifiedName::methodName(...)` prefix, including qualified and quoted
  names, keyword method names, comments, function warnings, and their original
  source positions.
- Located invalid `::` operators at their original line and column without
  changing Generic or Hive behavior.

### Verification

- Added focused Rust and public Python regressions for the reported multiline
  query, LF/CRLF positions, malformed neighbors, valid static method calls,
  warning positions, and dialect isolation.
- Verified the 15-case focused corpus against `io.trino:trino-parser:483` on
  Temurin JRE 25.0.4.1 with zero expected-label mismatches. The full suite now
  collects 5,015 pytest cases and runs 87 Rust unit tests.

## [0.20.0] - 2026-09-27

### Added

- Added a 100-case offline Trino 483 grammar corpus with exact validity,
  warnings, and error anchors, plus an independently verified 84-cell
  family/polarity/wrapper matrix.
- Added an honest Trino 483 inventory of 144 parser rules and 708 top-level
  alternatives, with separate fingerprints for the grammar, `SqlParser`,
  `PostProcessor`, and `AstBuilder`, exact source/corpus identity gates, and
  coverage for direct `createExpression` and row-pattern entry points.
- Added a dev-only JDK 25 Trino parser oracle with protocol schema 2 and a
  1,040-case project corpus exporter. Reports preserve raw/prepared input hashes,
  preparation metadata, entry points, parser error classes and locations, Java
  runtime identity, harness/class hashes, and every dependency-JAR hash. Java is
  not a runtime or package-build dependency.
- Added a no-Python Rust-core build and three `cargo-fuzz` targets for validation,
  statement analysis, and warning extraction, with 18 committed seeds, a 28-cell
  LF/CRLF transformation matrix, and six native-labelled deletion mutations.
- Added 30 single-statement invalid data-mart fixtures with exact diagnostics;
  29 remain parser-valid with warnings and one is now a parser error.

### Fixed

- Enforced Trino's LISTAGG-only `WITHIN GROUP` grammar and preserved valid
  LISTAGG overflow/filter/window forms.
- Added table-function descriptors and copartition forms, SQL/JSON encodings,
  JSON_TABLE plans, pattern-recognition windows and relation patterns, partial
  CASE predicates, MATCH/UNIQUE predicates, and array wildcard subscripts.
- Preserved nested function/type warnings and original source coordinates across
  all new compatibility paths. The pinned positive audit is now 484/484
  statements, 238/238 expressions, and 68/68 types.

### Verification

- The current suite contains 4,994 pytest cases and 86 Rust unit tests; the
  553-statement positive fixture corpus and 236 independent negatives are
  unchanged.
- Local bounded fuzz smoke completed without crash or timeout: 26,626 validation,
  23,395 analysis, and 21,328 warning-extraction iterations. The scheduled job
  repeats all targets with 60-second, 64-KiB input, 1-GiB RSS, and 10-second
  per-input timeout limits and uploads corpora/findings.
- `validate_batch()`/GIL work (V020-08) and performance profiling (V020-09)
  remain explicitly deferred and are not part of the v0.20.0 contract.

## [0.19.0] - 2026-09-26

### Added

- Added a pinned, optional SQLGlot 30.19.0 differential-audit tool and baseline
  for all 553 positive fixtures, 236 negative fixtures, and the extracted Trino
  483 parser corpus. SQLGlot outcomes distinguish full ASTs, `Command` fallback,
  parse errors, and token errors without adding a production dependency.
- Added scheduled CI coverage for the pinned SQLGlot baseline and an optional,
  non-blocking manual drift comparison against SQLGlot `main`.
- Audited every independently valid project SQL statement against the native
  Trino 483 parser and documented intentional forward-compatibility differences.
- Ported 27 native-Trino-verified cases from previously uncovered syntax
  families in SQLGlot's Trino tests, with pinned source provenance and five
  paired negative regressions.
- Added 30 data-mart SQL fixtures verified independently by the native Trino 483
  parser and by the public validator without catalog warnings.

### Fixed

- Accepted Trino CTAS option ordering with `COMMENT`, table properties,
  `AS query`, and terminal `WITH [NO] DATA`, preserving warnings in properties
  and the query.
- Parsed column default literals without consuming a following `NOT NULL`, so
  `ALTER TABLE ... ADD COLUMN ... DEFAULT NULL NOT NULL` and its numeric,
  string, typed, and interval neighbors validate correctly.
- Rejected external routine dollar bodies that do not begin with a newline,
  empty `ROLLUP()`/`CUBE()` grouping elements, and unquoted scalar calls to
  those grouping keywords, matching the native Trino parser.
- Accepted `JSON_QUERY` `KEEP/OMIT QUOTES [ON SCALAR STRING]` clauses and
  contextual routine labels named `iterate`, `leave`, or `set`; rejected
  single-quoted property keys accepted only by SQLGlot's `Command` fallback.
- Replaced the unsupported `QUALIFY` data-mart query with a CTE/window filter and
  corrected IP-network expressions to documented CIDR strings and IPADDRESS casts.

## [0.18.0] - 2026-09-25

### Added

- Added a 236-case independently executed invalid-SQL fixture contract; every
  case is confirmed invalid by the Trino 483 parser and must return an invalid
  result without warnings.
- Added focused Rust and Python regressions for parser-special functions,
  statement delimiters, reserved identifier roles, current-value expressions,
  row-pattern quantifiers, and source-located structural errors.

### Fixed

- Rejected 35 confirmed syntax false accepts, including trailing projection
  commas, missing JOIN criteria and DML keywords, malformed FETCH/SQL-JSON/
  MATCH_RECOGNIZE forms, unsupported operators and literals, invalid prepared
  statements, and empty statement segments.
- Enforced Trino reserved words in relation, column-definition, column-reference,
  dereference, DDL object, prepared-statement, routine, row-field, `JOIN USING`,
  and function-name roles while preserving every double-quoted form.
- Matched Trino parser-time rules for `IF`, `NULLIF`, `COALESCE`, `TRY`, and
  `FORMAT`, including row-pattern processing modes and qualified wildcards,
  without introducing general function signature validation.
- Checked every CTE in a `WITH` list and validated `MATCH_RECOGNIZE` measure
  aliases structurally, avoiding false rejects for non-reserved words inside
  measure expressions.
- Required source locations for 234/236 independent negative cases; the two
  locationless cases are explicitly documented upstream-parser limitations.
- Accepted row-pattern `{,}` / `{,}?`, `EXECUTE IMMEDIATE`, `GROUP BY AUTO`,
  `OFFSET ... FETCH`, and `OVER` as a contextual implicit projection alias;
  the latter now produces the existing non-fatal `AliasWarning`.

## [0.17.0] - 2026-09-24

### Fixed

- Rejected all 83 Trino reserved keywords when used as unquoted projection,
  table, subquery, CTE, or alias-column names, with source locations and no
  advisory warning for invalid SQL.
- Preserved every Trino non-reserved keyword as an explicit or unambiguous
  implicit alias, including `PARTITION`, `MATCH`, and `TABLESAMPLE`, without
  consuming real `LIMIT`, `OFFSET`, `FETCH`, `WINDOW`, `PIVOT`,
  `MATCH_RECOGNIZE`, or `TABLESAMPLE` clauses.
- Rejected single-quoted aliases while retaining identifier-prefixed typed
  literals, and preserved double-quoted reserved identifiers in DDL object
  names, aliases, and qualified column references.
- Accepted the Iceberg parser fixture's 247 Trino statements, including
  parenthesized `ARRAY` column types, top-level `TABLE` queries, table comments
  followed by properties, and SQL/JSON value, object, and wrapper clauses.
- Made the fixture inventory retain invalid positive statements instead of
  silently filtering them through the validator before parametrization.

## [0.16.0] - 2026-09-23

### Added

- Added the non-fatal `AliasWarning` and `ambiguous_aliases` result property for
  unquoted contextual aliases named `ALL`, `OVER`, `PARTITION`, `RETURN`, or
  `AT`, with original source positions.

### Fixed

- Accepted `AT` as a Trino non-reserved identifier instead of treating every
  occurrence as a temporal operator, while retaining `AT TIME ZONE`, `AT LOCAL`,
  and malformed-temporal-expression validation.

## [0.15.0] - 2026-09-19

### Fixed

- Recognized Trino's hidden `fail` and `combine_hash` built-in functions so
  valid calls no longer produce unknown-function warnings.

## [0.14.0] - 2026-09-11

### Added

- Added pre-parse complexity budgets for total tokens, per-statement tokens,
  nested groups, and routine blocks, with process-isolated crash regressions.
- Added opt-in `analyze_statements()` metadata with source spans, zero-based
  statement indexes, source-derived kinds, EXPLAIN/PREPARE inner kinds, and an
  error-statement index where a parser location is available.
- Added reproducible Trino 483 function/type catalogs with resolved source SHA,
  per-file checksums, read-only `--check`, and documented Geometry,
  SphericalGeography, and BingTile types.
- Expanded fixture and upstream gates to independently run all 276 positive
  fixture statements, a 57-case composition matrix, Java text blocks, and the
  executable Functions/Routines subset.

### Fixed

- Preserved function/type warnings and exact source positions across nested ROW
  and postfix ARRAY types, SQL/JSON clauses, wrapped/custom statements, CALL and
  ALTER EXECUTE expressions, routines, and full Iceberg time-travel expressions.
- Limited inline-function exemptions to their query and unqualified name, and
  enabled WITH FUNCTION/WITH SESSION under their supported wrappers.
- Rejected non-Trino CREATE INDEX, QUALIFY, LIMIT expressions/comma syntax,
  UPDATE FROM, DELETE USING, DML RETURNING, ILIKE, `<=>`, bare table functions,
  non-literal column defaults, and malformed CALL/ALTER EXECUTE forms. These are
  intentional false-accept corrections for `dialect="trino"`; generic behavior
  is unchanged.
- Accepted Trino non-reserved `limit`/`offset` identifiers, full
  `FOR VERSION AS OF <valueExpression>`, and postfix ARRAY types consistently.

Procedure existence, parameter names, arity/types, connector capabilities, and
other semantic checks remain outside the offline syntax contract.

## [0.13.0] - 2026-09-11

### Added

- Completed the pinned Trino 483 parser matrix: all 456 extracted statement
  cases and all 68 extracted type cases now validate, while all 23 direct
  negative statements remain rejected.
- Added current Trino role and privilege statements, nested-column `ALTER`,
  CTAS aliases and data disposition, table `LIKE`/column properties, `ANALYZE`
  properties, view comment/security options, scalar `JSON_TABLE` clauses, and
  structural SQL routine bodies.
- Added an Apache-attributed regression matrix with stable work-item IDs and
  resource-safety coverage for backtracking, excessive nesting, BOM/CRLF,
  comments, escaped strings, and adjacent Jinja templates.

### Fixed

- Made unambiguous Trino-only statement prefixes strict, with balanced groups
  and trailing-token checks instead of permissive parser fallback.
- Preserved function/type warning traversal and original source locations for
  compatibility-parsed statements, expressions, types, and routines.
- Bounded parser nesting and converted unexpected Rust parser panics into an
  invalid result at the PyO3 boundary.

## [0.12.0] - 2026-09-10

### Added

- Added compatibility parsing for query-scoped expression-returning `WITH
  FUNCTION` declarations, including multiple declarations and a following CTE
  query. Locally declared functions no longer produce unknown-function warnings.
- Added Trino query forms that `sqlparser-rs` does not parse directly:
  `CORRESPONDING [BY (...)]`, `PIVOT ... GROUP BY`, `NEAREST`, `ROW(...).*`
  with output aliases, `GROUP BY ALL/DISTINCT`, `AT LOCAL`, scalar relation
  `VALUES`, and empty `ROLLUP()` / `CUBE()` grouping elements.
- Added a located-token compatibility layer for the supported `PREPARE`,
  `ARRAY(type)`, `IPADDRESS`, Iceberg time-travel, `VALUES`, and identifier
  forms. It leaves comments and string literals untouched while retaining
  function/type warning locations.

### Fixed

- Preserved unknown-function and unknown-type discovery inside inline function
  declarations, row expansions, grouping expressions, and scalar `VALUES`
  relations with their original line and column positions.
- Rejected false accepts for empty CTAS column lists, incomplete `TABLESAMPLE`,
  numeric pseudo-typed literals, `WHERE FROM`, and `COUNT(DISTINCT *)`.
- Extended the pinned Trino 483 audit to 371 accepted positive statements while
  retaining rejection of all 23 direct negative statements and 52 of 56
  error-suite inputs. Remaining differences are documented in the parser plan.

## [0.11.0] - 2026-09-10

### Added

- Added a reproducible audit tool for pinned Trino 483 and PrestoDB 0.299 parser
  test corpora, plus a detailed parser-fidelity plan for future work.
- Added Trino `WITH SESSION` queries with multiple expression-valued session
  properties while preserving function-warning source locations.
- Added syntax support for hexadecimal, octal, and binary integer literals with
  Trino digit separators.
- Added current `CREATE/DROP CATALOG` and `CREATE/DROP BRANCH` forms, Trino
  `SHOW ... LIKE ... ESCAPE` variants, and `SHOW FUNCTIONS FROM/IN`.

### Fixed

- Replaced global Iceberg `@branch` rewriting with a token-aware transform that
  only applies to DML targets and never alters strings, comments, or arbitrary
  `@` syntax.
- Matched Trino identifier restrictions for ASCII unquoted identifiers,
  backquotes, empty quoted identifiers, and digit-leading identifiers.
- Rejected previously accepted invalid statement forms including parenthesized
  `ALTER ... SET PROPERTIES`, over-qualified `SET PATH`, `EXPLAIN VERBOSE`
  without `ANALYZE`, incomplete `SHOW ... ESCAPE`, and invalid branch options.

## [0.10.0] - 2026-09-10

### Added

- Added a complete SQL fixture contract and a Trino 483 documentation-derived
  syntax matrix covering general SQL, Iceberg, Hive/HDFS locations, table
  functions, and deeply nested structural types.
- Added Iceberg `FOR TIMESTAMP AS OF` syntax support for timestamp and date
  expressions.
- Added Trino `GRACE PERIOD`, `WHEN STALE`, and `COMMENT` options for
  `CREATE MATERIALIZED VIEW`.
- Added the documented `SUBSET` clause order for `MATCH_RECOGNIZE`.

### Fixed

- Stopped reporting table-function syntax, row-pattern navigation functions,
  and Iceberg `table_changes` as unknown functions.

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
- Added syntax support for deeply nested Trino `ROW` types, including
  `ARRAY(ROW(...))` and `MAP(..., ROW(...))`, while preserving source positions
  and leaving `ROW(...)` value constructors unchanged.

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
