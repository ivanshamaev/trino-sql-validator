# Roadmap

## v0.1.0 (initial release)
- sqlparser-rs based validation (see `plan.md`).
- Public API: `validate`, `validate_file`, `ValidationResult`, `Error`.
- Wheels for Linux (x86_64, aarch64), macOS (arm64, x86_64), Windows (x86_64).
- PyPI publishing via GitHub Actions + Trusted Publishing.

## v0.2.0
- Trino function-name validation: AST walk via `sqlparser::visitor`; unknown
  functions are surfaced as non-fatal `FunctionWarning` in `ValidationResult.warnings`.
  Catalog (459 names) auto-generated from Trino docs and committed in `src/functions.rs`.
  Only active for `dialect="trino"`.

## v0.3.0 — Trino statement coverage + data-type validation
- `TrinoDialect` now handles the Trino-only statement surface from
  `docs/src/main/sphinx/sql` (85/85 gap corpus passing): catalog/branch/function
  DDL, `ALTER TABLE/VIEW/MATERIALIZED VIEW`, `RESET SESSION`, `SET PATH`,
  `SHOW CREATE ...`, `DESCRIBE INPUT/OUTPUT`, role `GRANT`/`REVOKE`, `PREPARE
  [name] FROM` normalization. See `plan/sql-coverage.md`.
- Warning tuple widened to `(kind, name, line, column)` where `kind` is
  `"function"` or `"type"`; new `TypeWarning` + `ValidationResult.unknown_types`
  validate data-type names against a generated `src/types.rs` catalog
  (`tools/extract_types.py`).
- `CREATE FUNCTION`, `DESCRIBE INPUT/OUTPUT` and `RESET SESSION` handled
  strictly (no permissive sqlparser fallback).

## v0.9.0 — fixture-driven Trino compatibility
- All expected-valid SQL fixtures parse, including the Trino example corpus
  and the 100-statement Iceberg demo.
- Added compatibility parsing for `IPADDRESS` literals, Iceberg time travel and
  named references, additional `VALUES` forms, and `ALTER TABLE ... EXECUTE
  ... WHERE`.
- Completed function-catalog coverage for date/time expressions, `grouping`,
  and SQL/JSON functions.
- Added token-level support for nested `ROW` types and their `ARRAY`/`MAP`
  containers while preserving source spans and ordinary `ROW(...)` value
  constructors. See `plan/sql-coverage.md`.

## v0.10.0 — documentation-derived coverage
- Added a complete fixture inventory contract and an executable syntax matrix based
  on the Trino 483 SQL, Iceberg, Hive, and HDFS documentation.
- Added Iceberg `FOR TIMESTAMP AS OF` parsing for timestamp/date expressions,
  materialized-view staleness options, and corrected false warnings for
  table-function syntax, row-pattern navigation, and Iceberg `table_changes`.
- Added the documented `MATCH_RECOGNIZE SUBSET` clause order with strict malformed
  definition checks.
- Confirmed existing support for `EXECUTE IMMEDIATE`, `LIMIT ALL`, `JSON_TABLE`,
  `GROUP BY AUTO`, `FETCH ... WITH TIES`, `TABLESAMPLE`, Hive bucket/partition
  syntax, HDFS locations, and deeply nested structural types.
- Remaining parser work is tracked in `plan/additional_dev_v0.10.0.md`: `WITH
  SESSION`, inline `WITH FUNCTION`, `PIVOT ... GROUP BY`, `CORRESPONDING`, and
  `NEAREST`.

## v0.11.0 — parser-fidelity hardening
- Added a reproducible differential audit of direct-string tests from pinned
  Trino 483 and PrestoDB 0.299 parser corpora. The Trino corpus is the fidelity
  target; PrestoDB is a comparison only and must not expand the dialect.
- Replaced unsafe global branch rewriting with token-aware Iceberg DML branch
  handling, tightened Trino lexical checks, and added Trino non-decimal integer
  literals.
- Added `WITH SESSION`, current catalog/branch forms, strict property and path
  validation, session authorization checks, and `SHOW ... LIKE ... ESCAPE` /
  `SHOW FUNCTIONS FROM/IN` forms.
- The pinned audit now rejects all 23 direct negative statements and accepts
  354 of 456 extracted positive statements. Remaining gaps are explicit:
  inline `WITH FUNCTION`, `PIVOT ... GROUP BY`, `CORRESPONDING`, `NEAREST`,
  broader statement grammar, expressions, types, and SQL routine bodies. See
  `plan/v0.11.0_SqlParser.md`.

## v0.12.0 — query compatibility expansion
- Completed the carried high-value query forms: `WITH SESSION`, inline
  expression-returning `WITH FUNCTION`, `CORRESPONDING`, `PIVOT ... GROUP BY`,
  and `NEAREST`. Inline declarations participate in warning discovery, while
  calls to locally declared names are recognized.
- Added structural support for `ROW(...).* [AS (...)]`, `GROUP BY
  ALL/DISTINCT`, `AT LOCAL`, scalar relation `VALUES`, and empty `ROLLUP()` /
  `CUBE()` forms. Each compatibility transform is token-located so warning
  positions remain tied to the source SQL.
- Migrated the remaining SQL-changing compatibility paths from regex rewriting
  to token/context transforms, avoiding changes inside strings and comments.
- Added strict guards for known syntax false accepts. The reproducible Trino 483
  audit now accepts 371/456 positive statements and rejects all 23 direct
  negatives; it rejects 52/56 inputs from the separate error suite. The retained
  mismatches and remaining table-function/routine grammar work are explicit in
  `plan/v0.11.0_SqlParser.md`.

## v0.13.0 — parser-plan completion
- Completed statement, expression, structural type, and SQL routine work from
  `plan/v0.11.0_SqlParser.md`, including strict malformed neighbors and source
  position preservation.
- The pinned Trino 483 direct-string audit is 456/456 statements, 231/232
  expressions (the residual item is an empty extractor artifact), 68/68 types,
  and 23/23 direct negative statements. The separate error suite remains 52/56:
  its four differences are the intentional empty-file API and three semantic
  checks outside syntax validation.
- Trino master at `b2581fb32fcb` is tracked separately from the release pin and
  passes all 458 extracted statement cases. PrestoDB
  0.299 remains a non-target comparison corpus.

## Next — clearer Trino cursor
- Cover connector `CALL` signatures structurally without claiming semantic argument
  validation.
- Surface statement *type* (SELECT/DDL/...) from the parsed AST to the Python
  result (currently we only count statements and validate the file-level
  outcome); this unlocks an `allow_ddl=False` flag and per-statement error
  indexes.

## Later ideation
- **Trino-exact grammar:** bundle/port Trino's own `trino-parser` (ANTLR4) grammar
  via `antlr4rust`/`antlr4_rust` crate, or vendor Trino's grammar files. Gives exact
  syntax, at the cost of maintaining a grammar fork. Trigger if sqlparser-rs gaps
  become a blocker.
- **Semantic lint config:** allow relying on a live Trino server (JDBC/presto-client)
  for full semantic validation behind a flag.
- **CLI:** `trino-sql-validate path/to/file.sql` using `[project.scripts]`.
- **Formatter/normalizer** output from round-trip (`ast[0].to_string()`).
- **Pre-commit hook** integration.

## Maturity gates
- Keep `cargo fmt` / `clippy` / `cargo test` / `pytest` green in CI at all times.
- Every public Python API addition ships with `.pyi` + tests + docs.
