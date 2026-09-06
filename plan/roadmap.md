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

## v0.2.x — clearer Trino cursor
- Enrich `TrinoDialect` overrides for commonly-mis-parsed Trino-specific syntax:
  - `EXECUTE IMMEDIATE`, `CALL` signatures
  - `LIMIT ALL`
  - Trino function names/special `SELECT ... FROM UNNEST(...)`.
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