# trino-sql-validator

Fast **Trino SQL syntax validator** — a Python library whose core is written in
Rust and compiled to a native extension via [PyO3] + [maturin].

Installable from PyPI:

```bash
pip install trino-sql-validator
```

## Quickstart

```python
from trino_sql_validator import validate, validate_file

# A string with one or many statements
result = validate("SELECT 1; SELECT * FROM t WHERE a > 0;")
assert result.valid
assert result.statement_count == 2

# Invalid SQL returns a value, never raises
result = validate("SELECT * FORM t")
assert not result.valid
print(result.error)          # e.g. "Expected: end of statement, found: FORM at line 1, column 10"
print(result.error.line)     # 1

# Validate a file
result = validate_file("queries.sql", dialect="trino")
```

Invalid SQL (and files containing it) is returned as a `ValidationResult`;
it is **not** raised as an exception. Only genuine misuse (unknown dialect,
unreadable file) raises.

### Dialects

- `"trino"` (default) — Trino-flavored with a custom override tuned for
  Presto/Trino syntax (`LIMIT ALL`, backslash escapes, etc.).
- `"hive"` and `"generic"` — offered as permissive alternates.

## Known limitations

`sqlparser-rs` (the parser we use) performs **syntax** validation, not semantic
analysis. It may accept SQL that Trino would reject at analysis time (unknown
columns/tables, duplicate columns), and it can reject exotic Trino-specific DDL.
For the overwhelming majority of SELECT/DDL statements the results are accurate.
See [`plan/roadmap.md`](plan/roadmap.md) for the path toward stricter Trino fidelity.

## Development

See [`AGENTS.md`](AGENTS.md) for setup, internal conventions, and release steps.
Key commands:

```bash
python3 -m venv .venv && source .venv/bin/activate
pip install -U pip maturin && pip install -e ".[dev]"
maturin develop          # build + install native ext into the venv
cargo test               # Rust tests
pytest -q                # Python tests
cargo fmt --check        # formatting
cargo clippy --all-targets -- -D warnings
```

## License

MIT

[PyO3]: https://pyo3.rs
[maturin]: https://maturin.rs