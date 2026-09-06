# Plan: `trino-sql-validator` — Rust-backed Python SQL validator for Trino

## 1. Goal

A Python library, installed via `pip install`, that validates Trino SQL syntax.
The core is written in Rust and compiled to a platform-specific extension module.

- Input: a `.sql` file (or a string) containing **one or many** statements.
- Output: a structured result (`valid` bool + per-statement details / error with position).
- Distribution: source distribution (sdist) + wheels (manylinux / macOS / Windows),
  published to PyPI.

## 2. Design decisions

| Topic | Decision | Rationale |
|---|---|---|
| Language | Rust core + Python wrapper | Performance, single wheel, no JVM |
| Binding | PyO3 + maturin | Current standard for pyo3 wrapping |
| SQL parser | `sqlparser-rs` (`GenericDialect` + small custom `TrinoDialect` overrides) | Rust-native; no Trino-specific dialect exists upstream, but Generic covers most Trino statements |
| Forks/portability | ABI3 wheels (`abi3-py310`) via pyo3 | One wheel per platform across many Python versions |
| Error type | Return `dict`-like result objects, not raising on invalid input | Cleaner API; raise only on non-SQL errors |
| Statement splitting | Parse whole file; count top-level statements; expose per-file errors | Matches "validate a file with N statements" |

> **Fidelity caveat (documented in README):** sqlparser-rs does syntax, **not** semantics.
> It will accept some queries Trino would reject at semantic analysis (unknown columns,
> missing tables, duplicate column names) and may reject exotic Trino-specific DDL.
> Fidelity is "good" for common DML/DDL; a dedicated `TrinoDialect` module is provided
> to close the most impactful grammar gaps (see `plan/roadmap.md`).

## 3. Package name and layout

Package/project name: **`trino-sql-validator`** (PyPI name; underscores are
disallowed for wheels/import contortions, so hyphen on PyPI, underscore in import).

```text
trino-sql-validator/
├── Cargo.toml                # Rust crate: pyo3 ext module + deps
├── pyproject.toml            # maturin build backend + [project] metadata
├── README.md
├── LICENSE
├── .gitignore
├── rust-toolchain.toml       # pin stable toolchain
├── AGENTS.md
├── plan/
│   ├── plan.md               # this file
│   ├── roadmap.md            # future work (Trino-exact grammar, etc.)
│   └── functions-validation.md # function-name validation plan (v0.2.0)
├── src/
│   ├── lib.rs                # #[pymodule] entry point
│   ├── functions.rs          # GENERATED Trino function catalog (459 names)
│   ├── dialects/
│   │   └── mod.rs            # TrinoDialect (impl Dialect trait)
│   └── error.rs              # Mapping ParserError -> Python exception info
├── tools/
│   └── extract_functions.py  # regenerates src/functions.rs from Trino docs
├── python/trino_sql_validator/
│   ├── __init__.py           # pure-Python public API (re-exports, __all__)
│   ├── _validator.pyi        # type stubs referencing the compiled _native module
│   └── py.typed              # PEP 561
├── tests/
│   ├── conftest.py
│   ├── test_validator.py     # pytest: public API behavior
│   └── fixtures/*.sql        # sample multi-statement + invalid files
└── .github/workflows/
    ├── ci.yml                # lint, fmt, clippy, cargo test, pytest on push/PR
    └── release.yml           # build wheels + sdist, upload to PyPI on tag
```

## 4. Public Python API (target)

```python
from trino_sql_validator import validate, validate_file, ValidationResult

# Validate a string containing one or more statements
result: ValidationResult = validate(
    "SELECT 1; SELECT * FROM t;",
    dialect="trino",          # "trino" | "hive" | "generic"
)

# Validate a UTF-8 file
result = validate_file(
    "path/to/query.sql",
    dialect="trino",
)

print(result.valid)            # bool — entire file valid
print(result.statement_count)  # int — number of top-level statements parsed
print(result.error)            # Error | None (message, line, column)
```

`Error` and `ValidationResult` are `dataclass`es in the pure-Python layer; the
tuple-shaped native results come from `src/lib.rs`. Per-statement kinds and an
`allow_ddl` flag are deliberately **future work** (see `roadmap.md`), because
statement-kind classification requires walking the parsed AST.

## 5. Rust internals (sketch)

```rust
// src/lib.rs
#[pymodule]
mod _native {
    fn validate(sql: String, dialect: &str) -> PyResult<(bool, usize, Option<String>, Option<usize>, Option<usize>)>
    fn validate_file(path: &str, dialect: &str) -> PyResult<...>
}

// src/dialects/mod.rs
#[derive(Default, Clone, Copy, Debug)]
pub enum SqlDialect { Trino, Hive, Generic }

impl SqlDialect {
    pub fn from_str(s: &str) -> Result<Self, ...>;
    pub fn parser(&self) -> Box<dyn Dialect + ...> {
        // constructs TrinoDialect{}, HiveDialect{}, or GenericDialect{}
    }
}

pub struct TrinoDialect; // impl sqlparser::dialect::Dialect
```

Core validation flow:

```rust
fn validate_sql(sql: &str, dialect: &SqlDialect, allow_ddl: bool) -> (*): Result<Outcome> {
    let parsed = Parser::parse_sql(&*dialect.parser(), sql);
    match parsed {
        Ok(stmts) => Ok(Outcome::valid(stmts.len())),
        Err(e) => {
            // e.g. ParserError::ParserError { msg, location: Some(Location { line, column }) } or TokenizeError
            Ok(Outcome::invalid(e))
        }
    }
}
```

ParserError mapping (sqlparser 0.62 JSON forms verified):

- `ParserError::ParserError { msg, location: Option<Location> }`
- `ParserError::TokenizeError(msg)`
- Others (unexpected EOF variants) → fall back to message + line/col if present.

## 6. Dependencies

`Cargo.toml`:

```toml
[dependencies]
pyo3 = { version = "0.29", features = ["abi3-py310"] }
sqlparser = { version = "0.62", default-features = true }

[lib]
name = "_native"          # exact: must match `#[pymodule] _native`
crate-type = ["cdylib"]

[profile.release]
lto = true
codegen-units = 1
strip = true

[tool.maturin]  # actually placed in pyproject.toml
```

> Note: `sqlparser 0.62` pulls in `recursive` for stack-overflow protection by
> default (fine for CDN). No serde/visitor features needed.

`pyproject.toml`:

```toml
[build-system]
requires = ["maturin>=1.7,<2.0"]
build-backend = "maturin"

[project]
name = "trino-sql-validator"
requires-python = ">=3.10"
... # description, readme, license, classifiers, keywords

[project.optional-dependencies]
dev = ["pytest>=7", "ruff", "mypy"]

[tool.maturin]
python-source = "python"
module-name = "trino_sql_validator._native"
features = ["pyo3/extension-module"]

[tool.pytest.ini_options]
addopts = "-ra -q"
```

## 7. Phased implementation tasks

### Phase 0 — Repo setup (done in this session)
- [x] `git init` (already a repo, branch `main`, no commits)
- [x] Create `plan/plan.md`, `plan/roadmap.md`
- [x] Create `AGENTS.md`
- [x] `.gitignore`, `rust-toolchain.toml`, `LICENSE`
- [x] Initial docs/README stub

### Phase 1 — Buildable package skeleton (done)
- [x] `Cargo.toml`, `pyproject.toml`, `src/lib.rs` with a `#[pymodule]` exporting `__version__` and the `validate`/`validate_file` `#[pyfunction]`s
- [x] Empty `python/trino_sql_validator/` package with `__init__.py` + `py.typed`
- [x] Verify `maturin develop` installs and `import trino_sql_validator` works
- [x] `maturin sdist` + `maturin build` produce artifacts; wheels install cleanly via pip

### Phase 2 — Validation logic (done)
- [x] Implement `TrinoDialect` (custom `Dialect` impl), `SqlDialect` enum + `FromStr`
- [x] Implement `validate`/`validate_file` core in `src/lib.rs`
- [x] Add `ValidationResult`/`Error` helpers in pure Python (line/col extraction in Rust)
- [x] Rust unit tests (valid multi-statement, invalid SQL, empty input, comments-only, backtick rejection, encoding)

### Phase 3 — Public Python API + typing (done)
- [x] Pure-Python `validate()`, `validate_file()`, `ValidationResult`, `Error` in `__init__.py`
- [x] `.pyi` type stubs + `__all__`
- [x] pytest suite (`tests/`) covering public API incl. files with multiple statements
- [x] Ruff + mypy clean

### Phase 4 — CI/CD (GitHub Actions) (done — adjust org/repo before first run)
- [x] `.github/workflows/ci.yml` — on push/PR: check(cargo fmt --check, clippy), test(cargo test), maturin build + pytest
- [x] `.github/workflows/release.yml` — on tag `v*`: matrix build wheels (manylinux x86_64/aarch64, macOS arm64/x86_64, Windows) + sdist; upload to PyPI
- [x] Trusted Publishing (OIDC) to PyPI + PyPI project setup notes

### Phase 5 — Release hygiene (before first tag)
- [ ] Point `Cargo.toml` `repository` + README badges at the real GitHub org/repo
- [ ] Once on PyPI: add `trino-sql-validator` trusted publisher entry for this repo
- [ ] Bump versions in `Cargo.toml` + `pyproject.toml` (single source of truth = `Cargo.toml`; keep in sync)
- [ ] Verify wheels on a clean Linux/macOS container (import + validate before tagging)

## 8. Versioning

- Semantic versioning (`X.Y.Z`).
- Source of truth: `Cargo.toml` `[package] version`. maturin reads it for the wheel
  version if `[project] version` is omitted (maturin merges `Cargo.toml` metadata).
  Document in AGENTS.md: always bump `Cargo.toml`, keep `pyproject.toml` consistent.
- Tags: `vX.Y.Z` trigger `release.yml`.

## 9. CI/CD details (GitHub Actions)

### ci.yml (on: push, pull_request)
```yaml
jobs:
  check:            # cargo fmt --check, cargo clippy -- -D warnings
  test:             # cargo test
  python:           # setup python 3.10..3.13, maturin develop, pytest, ruff, mypy
```

### release.yml (on: push tags v*)
Matrix (GitHub-hosted runners + maturin-action cross-compilation):

| OS | target(s) | manylinux |
|---|---|---|
| ubuntu | x86_64 | 2_17 |
| ubuntu | aarch64 (cross) | 2_17 |
| macos | arm64, x86_64 | — |
| windows | x86_64 | — |

Each step:
```yaml
- uses: PyO3/maturin-action@v1
  with:
    target: <matrix>
    manylinux: 2_17
    args: --release --out dist --interpreter 3.12
- name: Upload sdist (linux only)
  uses: PyO3/maturin-action@v1
  with:
    command: sdist
    args: --out dist
```

Publishing:
```yaml
- uses: pypa/gh-action-pypi-publish@release/v1
  with:
    attestations: true          # optional provenance
    password: pypi               # Trusted Publishing via OIDC (no token value)
```
PyPI Trusted Publishing requires:
1. PyPI account → project (`trino-sql-validator`) → "Trusted publishers" → add
   GitHub repo `OWNER/trino-sql-validator` + workflow name `release.yml`.
2. Do **not** store any token as a secret.
Hardening (recommended): pin maturin-action & maturin version, use
`--compatibility pypi`, `--locked`, `strip=true`, OIDC.

## 10. Testing strategy

Two layers (per PyO3 best practice):
1. **Rust** (`src/**/*.rs #[cfg(test)]`, `cargo test`): pure parser logic — fast.
2. **Python** (`tests/test_validator.py`, `pytest`): exercise the real extension
   through the public API, including multi-statement files.

Sample fixtures (`tests/fixtures/`):
- `valid_multi.sql` — 3 statements separated by `;`
- `invalid_one.sql` — one obviously bad statement (e.g. `SELEC 1;`)
- `valid_single.sql`
- `empty.sql`, `comment_only.sql`
- `ddl.sql` — `CREATE TABLE`, `DROP TABLE`, `INSERT INTO`, etc.

Edge cases to cover:
- trailing `;` and missing final `;`, whitespace-only file → 0 statements valid
- dialect switching behavior (trino is strictest: rejects backquoted identifiers)
- multi-byte/unicode content, BOM, CRLF line endings (line/column correctness)
- very long/deeply nested statements (no stack overflow → parser stack protection)

## 11. Definition of done (for v0.1.0)

- [ ] `pip install trino-sql-validator` works on Linux/macOS/Windows
- [ ] `validate()` and `validate_file()` correctly classify valid/invalid multi-statement SQL
- [ ] Errors report message + line/column
- [ ] Type stubs + `py.typed` present; mypy/ruff clean
- [ ] Rust tests + pytest green in CI
- [ ] release.yml builds & publishes wheels + sdist to PyPI on `v*` tag
- [ ] README documents fidelity limitations + quickstart