# trino-sql-validator

Fast **Trino SQL syntax validator** — a Python library whose core is written in
Rust and compiled to a native extension via [PyO3] + [maturin].

Installable from PyPI:

```bash
pip install trino-sql-validator
```

## Quickstart

```python
from trino_sql_validator import analyze_statements, validate, validate_file

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

# Opt-in per-statement metadata; indexes are zero-based
analysis = analyze_statements("EXPLAIN SELECT 1; CALL system.custom_proc()")
assert analysis.validation.valid
assert analysis.statements[0].kind == "explain"
assert analysis.statements[0].inner_kind == "query"
assert analysis.statements[1].kind == "call"
```

Invalid SQL (and files containing it) is returned as a `ValidationResult`;
it is **not** raised as an exception. Only genuine misuse (unknown dialect,
unreadable file) raises.

### Advisory warnings

For `dialect="trino"`, `validate()` also checks that every function called and
every data type used in the SQL exists in the documented Trino catalog. Unknown
names are reported as non-fatal `warnings` — `valid` stays `True` because syntax
is fine:

```python
result = validate("SELECT marh(1.5)")       # round() misspelled
assert result.valid
print(result.warnings)                      # (FunctionWarning(name='marh', line=1, column=8),)
print(result.unknown_functions)             # ['marh']

result = validate("CREATE TABLE t (a bignum, b bigint)")  # bigint vs bignum
print(result.warnings[0])                   # TypeWarning(name='bignum', line=1, column=19)
print(result.unknown_types)                 # ['bignum']
```

The contextual words `ALL`, `OVER`, `PARTITION`, `RETURN`, and `AT` are valid
non-reserved Trino identifiers, but are easy to confuse with surrounding SQL
syntax. Using one as an alias therefore produces a non-fatal `AliasWarning`;
quote the alias to make the identifier intent explicit and suppress the warning:

```python
result = validate("SELECT orderdate AS At")
assert result.valid
print(result.warnings[0])                   # ambiguous unquoted alias 'at' at line 1, column 21
print(result.ambiguous_aliases)             # ['at']

assert validate('SELECT orderdate AS "At"').warnings == ()
```

`AT` is also used by the temporal operators `AT TIME ZONE` and `AT LOCAL`.
Those operator forms do not produce alias warnings.

Trino's 83 reserved keywords are stricter: an unquoted reserved alias is a
syntax error, including after an explicit `AS`. Double quotes turn the word
into a valid delimited identifier in aliases, object names, and column
references; single quotes do not:

```python
assert not validate("SELECT 1 AS where").valid
assert validate('SELECT 1 AS "where"').valid

assert validate('CREATE TABLE dwh_team."FROM" AS SELECT 1 AS "ALTER"').valid
assert validate(
    'SELECT "FROM"."ALTER" FROM dwh_team."FROM" AS "FROM"'
).valid
```

The catalogs are auto-generated from the Trino docs and only check *name
existence*, not argument counts, precision/scale, or semantic correctness.
`hive`/`generic` dialects skip these checks. False positives are possible if a
deployed Trino adds plugin functions/types beyond the docs.

Inline `WITH FUNCTION` names are exempt only within their own query scope.
Qualified calls with the same final name are still checked. Procedure names in
`CALL` and `ALTER TABLE ... EXECUTE` are not scalar functions and therefore do
not produce `FunctionWarning`; their existence, parameters, arity, permissions,
and connector availability require a Trino coordinator and are out of scope.

### Statement metadata

`analyze_statements()` is an opt-in API that returns the unchanged
`ValidationResult` together with a tuple of `StatementInfo`. Each entry contains
a zero-based index, source span, source-derived kind, and (for `EXPLAIN` or
`PREPARE`) an `inner_kind` when it can be identified. On invalid multi-statement
input, `error_statement_index` identifies the source statement when the parser
provided a location. Existing `validate()` and `validate_file()` return types
are unchanged.

### dbt and Jinja templates

Jinja/dbt SQL is supported by default. `validate()` and `validate_file()` use
`jinja="auto"` to mask Jinja expressions, statements, and comments before
parsing while preserving line numbers and file structure. This supports
constructs such as `{{ ref("orders") }}` and `{{ var("catalog") }}` without
requiring a dbt installation or project context. Use `jinja="mask"` as an
explicit spelling of the same mode.

Use `jinja="reject"` to pass the original template directly to the SQL parser.
Masking cannot determine SQL generated by control-flow blocks, macros, or
adapter semantics; render those cases with dbt and validate the rendered SQL
for complete coverage.

### Dialects

- `"trino"` (default) — Trino-flavored with a custom override tuned for
  current Trino syntax, including Iceberg branches/time travel, complex nested
  types, routines, table functions, SQL/JSON, and Trino-specific DDL.
- `"hive"` and `"generic"` — offered as permissive alternates.

## Known limitations

`sqlparser-rs` (the parser we use) performs **syntax** validation, not semantic
analysis. It may accept SQL that Trino would reject at analysis time (unknown
columns/tables, duplicate columns), and it can reject exotic Trino-specific DDL.
The validator has targeted compatibility parsing for documented Trino syntax,
including nested `ROW`/`ARRAY`/`MAP` types, but it does not replace Trino's
semantic analyzer. See [`plan/roadmap.md`](plan/roadmap.md) for the path toward
stricter Trino fidelity.

Parser fidelity is checked reproducibly against direct-string cases extracted
from Apache Trino's parser tests. With ordinary Java strings and text blocks,
the pinned Trino 483 audit currently accepts 481/484 statements, 231/238
expressions, 68/68 types, the extracted Functions/Routines subset, and rejects
23/23 direct negative statements. Known differences are pinned in a named
allowlist; new mismatches or a reduced extracted denominator fail the audit.
These figures and the 553 independently checked positive fixture statements
describe measured corpora, not complete Trino grammar or connector behavior.
SQL embedded inside ordinary string literals, JSON paths, WKT, dynamic SQL, and
unrendered macro output is intentionally opaque rather than recursively parsed.

An optional offline differential audit compares the same fixtures and Trino 483
corpus with pinned SQLGlot 30.19.0. SQLGlot is neither a runtime dependency nor
an alternative validity backend; its full-AST, `Command` fallback, and error
outcomes are tracked separately to identify candidates for native improvements.

To keep invalid or adversarial input from exhausting the native parser stack,
validation rejects a statement after 4,096 significant SQL tokens, nesting
deeper than 256 groups or routine blocks, and an input after 65,536 significant
tokens. The failure is returned as an ordinary invalid `ValidationResult`; when
the limiting token has a source position, that position is included in the
error. Semicolon-separated statements have independent per-statement budgets.

## Development

See [`AGENTS.md`](AGENTS.md) for setup, internal conventions, and release steps.
The current automated suite contains 4,748 pytest cases and 82 Rust unit tests.
Key commands:

```bash
python3 -m venv .venv && source .venv/bin/activate
pip install -U pip maturin && pip install -e ".[dev]"
maturin develop          # build + install native ext into the venv
cargo test               # Rust tests
pytest -q                # Python tests
cargo fmt --check        # formatting
cargo clippy --all-targets -- -D warnings
# optional differential parser audit
pip install -e ".[sqlglot-audit]"
python tools/audit_sqlglot.py --baseline \
  plan/sqlglot_30_19_0_trino_483_audit_baseline.json --fail-on-regression
python tools/extract_functions.py --ref 483 --check
python tools/extract_types.py --ref 483 --check
python tools/audit_upstream_parsers.py --baseline plan/trino_483_audit_baseline.json --fail-on-regression
```

## License

MIT

[PyO3]: https://pyo3.rs
[maturin]: https://maturin.rs

## Comparison with SQLGlot

`trino-sql-validator` and [SQLGlot](https://github.com/tobymao/sqlglot) solve
different problems. This library focuses on strict, fast validation of Trino
syntax and returns a small validation result with Trino-specific catalog
warnings. SQLGlot provides a rich AST and is a better fit for formatting,
rewriting, lineage, and translation between SQL dialects, but its parser is
deliberately permissive and is not a strict Trino validity oracle.

The reproducible audit below compares `trino-sql-validator 0.19.0` with
SQLGlot 30.19.0 against project fixtures and parser cases extracted from Trino
483. “SQLGlot accepted” includes its `Command` fallback, which preserves an
unsupported statement as text without fully parsing it. “Full AST” excludes
that fallback.

| Audit set or capability | `trino-sql-validator` | SQLGlot |
| --- | --- | --- |
| 553 valid project fixture statements | 553 accepted | 544 accepted; 441 produced a full AST |
| 236 invalid project fixture statements | 236 rejected | 132 produced an error; 22 became `Command`; 82 produced an AST and were accepted |
| 484 valid Trino 483 statements | 481 accepted | 454 accepted; 255 produced a full AST |
| 238 valid Trino 483 expressions | 231 accepted | 221 accepted |
| 68 valid Trino 483 types | 68 accepted | 48 accepted |
| 23 directly invalid Trino 483 statements | 23 rejected | 5 produced a parse/token error; the other 18 became `Command` |
| Primary use | Trino syntax validation, source locations, statement metadata, function/type warnings | Multi-dialect AST, transformation, generation, optimization, and lineage |
| Runtime | Rust native extension with parser resource limits | Python, with an optional compiled distribution |
| Invalid-input API | Returns `ValidationResult`; invalid SQL does not raise | Normally reports `ParseError`/`TokenError`, depending on the selected error level |
| Jinja/dbt input | Length-preserving masking is built in | No equivalent contract in the audited parse path |

The measured corpora are regression benchmarks, not proof of complete Trino
grammar coverage. SQLGlot remains an optional offline audit dependency and is
not used by `validate()` at runtime. Confirmed SQLGlot findings are checked
against the native Trino parser before being implemented in the Rust parser.
See the [full SQLGlot audit](plan/v0.19.0_sqlglot_parser.md) for methodology,
known mismatches, and detailed results.
