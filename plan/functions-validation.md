# Function-name validation for Trino

Status: implemented for v0.2.0.
Source of truth for the catalog: Trino docs at
`docs/src/main/sphinx/functions/` in `trinodb/trino` (MyST, current format).

## Goal

Catch a common class of error today's validator ships past: a syntactically
well-formed SQL statement that calls a function Trino does not provide (typo,
wrong engine, e.g. `SELECT marh(x)` vs `round(x)`). `sqlparser-rs` treats any
`name(...)` as a function call, so it never flags unknown names.

## Where the catalog comes from

The `functions/` tree has 35 topic files (`aggregate.md`, `string.md`, ...) plus
`list.md`. A function name appears in two machine-greppable forms:

- `:::{function} name(args) -> type` directives (the authoritative signatures,
  one per overload).
- `{func}\`name\`` cross-references in `list.md` (the alphabetical catalog of
  every documented function).

Our catalog = **union** of directive names across all topic files AND `{func}`
names in `list.md`. Current Trino docs yield **459 canonical names**
(measured 2026-09-06). This catches all of `count/sum/avg/min/max/...` plus
Trino-only functions.

Cast/keywords that sqlparser models specially (`cast`, `try`, `if`, `coalesce`)
never reach the function walker, so they need no special-casing.

## Algorithm (time-optimal)

1. **No extra cost on invalid SQL.** Parse first. If `Parser::parse_sql` fails,
   return the existing error today and skip function walking entirely.
2. **Single linear AST walk.** For a successful parse, walk every statement once
   with `sqlparser`'s `visit_expressions` (`feature = "visitor"`). For each
   `Expr::Function`, look up the *last* `Ident` of its `ObjectName` in a static
   `HashSet<&'static str>` of canonical names. O(functions-in-query), O(1) lookup.
3. **Exact line/column for free.** `Ident.span` gives byte-exact start
   `Location` — no regex, no string scanning (unlike the error-location path).
4. **Static, lock-free data.** The name list is committed to `src/functions.rs`
   as a `LazyLock<HashSet>`. Built once by `tools/extract_functions.py` (a
   dev-only script that pulls the docs). No runtime/build-time network.

The name list only changes when Trino adds/removes functions (a release-scale
event). Committing the generated list keeps the crate offline-buildable and
deterministic; regenerating is opt-in (`python tools/extract_functions.py`).

## Behavior / semantics

- Only `dialect == "trino"` checks functions. `hive`/`generic` stay as-is
  (permissive escapes hatch; their function surface differs from Trino's).
- A call to an unknown function does **not** make the statement "invalid" in the
  existing `valid: bool` sense — that stays strictly about syntax. It is
  reported as a `warning` alongside the syntax result. This preserves the
  "bad syntax is a value, not an exception" contract on parse errors and keeps
  the truly-syntactic check peer-reviewable.
- Stopping early: the walker records unknown names and their locations and can
  short-circuit (via `ControlFlow::Break`) to avoid scanning the rest of a large
  script once already-warned — bounded memory, unchanged O(n) time.
- Fully-qualified calls (`schema.round(x)`): match on the bare function name;
  the leading qualifier is a schema reference, not part of the function.

## API shape

`ValidationResult` gains an optional `warnings: tuple[FunctionWarning, ...]`.

```python
@dataclass(frozen=True)
class FunctionWarning:
    name: str
    line: int | None = None
    column: int | None = None
```

Rust `validate`/`validate_file` return a 6-tuple
`(valid, statement_count, error_message, line, column, warnings)` where
`warnings` is a list of `(name, line, column)` triples. pyo3 converts the
plain `Vec<(String, Option<usize>, Option<usize>)>` to a Python list of tuples
directly — no serialization dependency needed.

## Files

- `tools/extract_functions.py` — dev-only doc parser → `src/functions.rs`.
- `src/functions.rs` — generated static catalog + `is_known_function()`.
- `src/lib.rs` — enable `visitor` feature usage; walk AST after successful parse.
- `python/trino_sql_validator/__init__.py` — `FunctionWarning`, `warnings` field.
- `python/trino_sql_validator/_native.pyi` — updated tuple type.
- `tests/test_validator.py` — unknown/known/qualified/trino-only cases.
- `tests/fixtures/` — sample queries with/without unknown functions.

## Verification

- `cargo test` (Rust unit tests for the walker + catalog sanity).
- `pytest` — unknown function → warning with correct line/column; known &
  multi-argument & qualified calls → no warning; `dialect="generic"` → no
  function checks; invalid SQL → parse error, no warnings.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `ruff`,
  `mypy` all stay green.

## Trade-offs / limits (documented, not "fixed" here)

- Catalog is the *documented* function set. Trino deploys may have
  connector/plugin-specific functions beyond docs → possible false positives for
  exotic setups. Warnings are non-fatal and per-dialect-trino, so this is safe.
- We validate name existence, not arity/type (sqlparser does not model Trino's
  function signatures beyond parse; arity/type checking is semantic and out of
  scope for a syntax validator). See roadmap for `trino-parser` on the far end.
