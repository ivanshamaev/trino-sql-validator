# AGENTS.md

Guidance for AI agents and humans working in `trino-sql-validator`.

## What this project is

A Python library, backed by Rust, that validates Trino SQL syntax.
Built with PyO3 + maturin. Distributed as wheels + sdist on PyPI.

Layout:
- `src/` — Rust crate (the compiled `_native` extension). `src/lib.rs` holds the
  `#[pymodule] fn _native`. `src/dialects/` holds the `TrinoDialect`/`SqlDialect`.
  `src/functions.rs` is the **generated** Trino function catalog (`@generated`).
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
- **Tools:** Rust stable (see `rust-toolchain.toml`), Python >= 3.10, maturin,
  pytest, ruff, mypy.

## Commands

```bash
# Set up
python3 -m venv .venv && source .venv/bin/activate
pip install -U pip maturin; pip install -e ".[dev]"   # installs build+test tooling

# Build & install the native extension into the venv (fast dev loop)
maturin develop

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
```

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

## CI/CD

- `.github/workflows/ci.yml` — fmt/clippy/test/pytest/ruff/mypy on every push & PR.
- `.github/workflows/release.yml` — builds wheels (manylinux, macOS, Windows) + sdist
  and publishes to PyPI when a tag `v*` is pushed. Uses PyPI **Trusted Publishing**
  (OIDC); no API tokens in constants.
- To cut a release: bump versions (see "Source of truth"), summarize in
  `CHANGELOG.md`, then `git tag vX.Y.Z && git push origin vX.Y.Z`. Do not push
  releases unless explicitly asked.

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
