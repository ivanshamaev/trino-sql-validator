# AGENTS.md

Guidance for AI agents and humans working in `trino-sql-validator`.

## What this project is

A Python library, backed by Rust, that validates Trino SQL syntax.
Built with PyO3 + maturin. Distributed as wheels + sdist on PyPI.

Layout:
- `src/` — Rust crate (the compiled `_native` extension). `src/lib.rs` holds the
  `#[pymodule] fn _native`. `src/dialects/` holds the `TrinoDialect`/`SqlDialect`.
  `src/functions.rs` and `src/types.rs` are the **generated** Trino catalogs
  (`@generated`); their provenance is recorded in the adjacent manifests.
- `python/trino_sql_validator/` — pure-Python public API (`__init__.py`), type
  stubs (`_native.pyi`), `py.typed`.
- `tests/` — pytest suite for the public API (+ `.sql` fixtures).
- `plan/` — planning docs (`plan.md`, `roadmap.md`, `functions-validation.md`).
- `tools/extract_functions.py` / `tools/extract_types.py` — regenerate the
  committed catalogs and source manifests from pinned Trino docs.

## Source of truth

- **Version:** single source of truth is `Cargo.toml` `[package] version`.
  Bump it there; keep `pyproject.toml` `[project]` consistent. Do NOT bump only one.
- **Public API:** defined once in `python/trino_sql_validator/__init__.py`; mirror
  the native functions via the `#[pyfunction]`s in `src/lib.rs`.
- **Native ABI:** Rust returns positional tuples (`ValidationResultTuple`,
  `StatementInfoTuple`, `StatementAnalysisTuple`). Their shapes are mirrored by
  aliases/conversion code in `python/trino_sql_validator/__init__.py` and by
  `python/trino_sql_validator/_native.pyi`; update all three together.
- **Tools:** Rust stable (see `rust-toolchain.toml`), Python >= 3.10, maturin,
  pytest, ruff, mypy.

## Commands

```bash
# Set up
python3 -m venv .venv && source .venv/bin/activate
pip install -U pip maturin; pip install -e ".[dev]"   # installs build+test tooling

# Build & install the native extension into the venv (fast dev loop)
maturin develop

# Targeted tests
pytest tests/test_validator.py::test_name -q
pytest -k substring
cargo test test_name

# Rust checks
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# Python checks
pytest -q
ruff check .
mypy python/trino_sql_validator

# Produce distributable artifacts
maturin build --release          # wheels
maturin sdist                    # source distribution

# Regenerate the Trino function/type catalogs from upstream docs
python tools/extract_functions.py --ref 483  # fetch pinned docs + write catalog/manifest
python tools/extract_functions.py --docs-path /path/to/trino/docs/src/main/sphinx/functions  # local checkout
python tools/extract_types.py --ref 483      # fetch pinned docs + write catalog/manifest
python tools/extract_types.py --docs-path /path/to/trino  # local checkout
python tools/extract_functions.py --ref 483 --check
python tools/extract_types.py --ref 483 --check

# Compare against the pinned upstream parser corpus
python tools/audit_upstream_parsers.py \
  --baseline plan/trino_483_audit_baseline.json --fail-on-regression
```

`pytest` imports the compiled extension from the active environment. After any
Rust change (including regenerated Rust catalogs), run `maturin develop` before
pytest or the suite can exercise a stale native build.

## Conventions / rules

1. **Rust-native only.** Do not introduce a JVM or a subprocess dependency. The
   parser is `sqlparser-rs`; add dialect overrides in `src/dialects/` rather than
   vendoring whole grammars.
2. **`_native` name is fixed** — `[lib] name = "_native"` in `Cargo.toml` must equal
   the `#[pymodule]` name. Do not rename without updating `pyproject.toml`
   (`module-name = "trino_sql_validator._native"`).
3. **Do not raise on invalid SQL.** `validate()`/`validate_file()` return a
   `ValidationResult`; the whole design treats bad syntax as a *value*, not an
   exception. Only real programming errors raise.
4. **Type-preserving Python.** Any function exposed to Python needs a `.pyi` stub
   and a `py.typed` marker. Keep the pure-Python `__init__.py` thin (re-export +
   convenience wrappers only).
5. **Comment-free Rust unless needed** per opencode style; keep docs in `plan/`
   and doc comments for public API only.
6. **Keep CI green.** `fmt`, `clippy -D warnings`, `cargo test`, `pytest`, `ruff`,
   `mypy` all must pass before merging. Do not skip clippy lints.
7. **Testing:** logic tests belong in Rust (`#[cfg(test)]`, `cargo test`), and
   integration tests through the real extension belong in `tests/*.py` (pytest).
   Adding a Python API feature without pytest tests is not done.
8. **Generated catalogs:** never hand-edit `src/functions.rs`, `src/types.rs`, or
   their `src/*.manifest.json` files. Change the extractor inputs/curated sets,
   regenerate, and run both catalog commands in `--check` mode.
9. **Fixture inventory:** every `tests/fixtures/*.sql` file needs matching entries
   in both `FIXTURE_EXPECTATIONS` and `FIXTURE_FEATURES` in
   `tests/test_fixture_inventory.py`. Positive fixtures are also validated one
   statement at a time, so keep validity, statement counts, expected warnings,
   and feature profiles exact.

## Architecture invariants

The data flow is Python → `_native` → dialect-specific parse → catalog warnings
→ positional tuple → frozen Python dataclasses.

- `python/trino_sql_validator/__init__.py` is a thin wrapper around `_native`.
  Its `_mask_jinja` Jinja/dbt masking must preserve input length and newlines so
  native source locations still refer to the original SQL. It validates public
  options and converts native tuples into frozen dataclasses.
- `src/lib.rs::validate_sql_impl` converts unexpected parser panics into an
  invalid result. The token and nesting budgets (`MAX_SQL_TOKENS`,
  `MAX_STATEMENT_TOKENS`, `MAX_NESTING_DEPTH`) and the Trino-only empty-`FROM`
  check run before parsing; do not bypass these safeguards when adding a parser
  path. Hive/Generic go directly through `sqlparser::Parser::parse_sql` and do
  not produce catalog warnings.
- There is no upstream `sqlparser` Trino dialect. Trino parsing in
  `src/dialects/trino_types.rs::parse_sql` tokenizes once, rejects invalid token
  shapes in `validate_trino_*` passes, and rewrites supported Trino-only syntax
  in place in `normalize_*` passes into forms accepted by the Generic grammar,
  while preserving token spans. It first tries cheaper rewrites; after a failed
  parse it applies heavier passes such as table-function arguments,
  `WITH SESSION`, inline `WITH FUNCTION`, and row expansions. Run
  `validate_trino_ast` on the final AST. New syntax normally needs a focused
  validate/normalize pass plus a pytest regression.
- If a normalization removes or replaces a fragment containing expressions or
  types (for example property values or pivot expressions), parse and return
  that fragment as `compatibility_metadata`; otherwise function/type warnings
  silently disappear from valid SQL.
- `src/dialects/trino_statements.rs` handles Trino-only statements without a
  `sqlparser` AST node, including Trino CREATE CATALOG/ROLE/BRANCH/FUNCTION,
  SHOW/DESCRIBE and SET PATH/ROLE variants, and Iceberg branch ALTER forms. A
  recognized shape returns a placeholder so it still counts as valid; `None`
  falls back to `sqlparser`. `analyze_statements()` derives kinds and spans from
  source tokens (`statement_kind`/`statement_info`), not placeholder AST nodes.
- `src/dialects/generic_delegates.rs` delegates all non-overridden `Dialect`
  hooks to `GenericDialect`, while Trino overrides statement parsing and lexing:
  unquoted identifiers are ASCII without `$`, quoted identifiers use double
  quotes rather than backticks, and string literals do not use backslash
  escapes. Its header names `tools/gen_generic_delegates.py`, but that generator
  is not committed; when upgrading `sqlparser`, compare the implementation
  manually with the new `Dialect` trait.
- Catalog warnings (`find_unknown_functions`/`find_unknown_types`) run only for
  Trino and walk the main AST, inline declarations, and compatibility metadata.
  Inline `WITH FUNCTION` names are exempt only in their own statement and only
  when unqualified. Pattern-recognition and table-function pseudo-functions are
  allow-listed in `is_known_trino_function`. Warnings are sorted by source
  position, deduplicated, and never change `valid`.
- The upstream audit imports the installed package. Without `--trino-root` or
  `--presto-root` it downloads pinned sources. Intentional result changes require
  reviewing/updating `plan/trino_483_audit_baseline.json` (using
  `--print-baseline`) and the measured coverage figures in `README.md`.

## CI/CD

- `.github/workflows/ci.yml` — fmt/clippy/test/pytest/ruff/mypy on every push & PR.
- `.github/workflows/release.yml` — builds wheels (manylinux, macOS, Windows) + sdist
  and publishes to PyPI when a tag `v*` is pushed. Uses PyPI **Trusted Publishing**
  (OIDC); no API tokens in constants.
- To cut a release: bump both version files (and let Cargo refresh `Cargo.lock`),
  summarize in `CHANGELOG.md`, run the full checks, commit, then create an
  annotated tag and push the branch and tag together, for example
  `git tag -a vX.Y.Z -m "Release vX.Y.Z"` followed by
  `git push --atomic origin main vX.Y.Z`. Do not push releases unless explicitly
  asked.

## Known limitation (keep in mind when working here)

`sqlparser-rs` does syntax, not semantics. It accepts some SQL Trino rejects at
semantic analysis and can reject exotic Trino-specific DDL. This is a documented,
accepted limitation (see README + plan/roadmap.md). Do not "fix" by loosening the
dialect to Generic by default for `dialect="trino"`.

Function validation (`ValidationResult.warnings`) checks only **name existence**
against the documented Trino catalog (`src/functions.rs`); it does not check
arity or argument types — that is semantic analysis, out of scope for a syntax
validator. False positives are possible if a deployed Trino has plugin functions
beyond the docs; warnings are non-fatal by design.

Before widening or narrowing accepted Trino grammar, review `plan/roadmap.md`,
the relevant per-version plans, and `plan/sql-coverage.md`; they record deliberate
syntax-versus-semantics boundaries and known compatibility decisions.
