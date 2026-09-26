# Trino SQL coverage and validation boundaries

Status: v0.19.0, Trino 483 pin, SQLGlot 30.19.0 comparison pin.

## Measured coverage

- All 78 SQL files in `tests/fixtures` have explicit outcome, statement-count,
  warning, and feature-profile expectations. All 553 statements from positive
  non-empty fixtures are also validated independently.
- The 30 files in `tests/fixtures/invalid_datamarts` are diagnostic fixtures:
  every file contains one statement and must return at least one catalog warning
  or a parser error. They are intentionally excluded from the positive corpus.
- The current automated suite collects 4,748 pytest cases and 82 Rust unit tests.
- `trino_invalid_sql.sql` contributes 236 independent parser-negative cases;
  each is passed to `validate()` separately rather than as multi-statement SQL.
  Trino 483 and v0.19.0 both reject all 236, compared with 201/236 rejections in
  v0.17.0.
- The 57-case composition matrix exercises query features under their supported
  root, EXPLAIN, PREPARE, CTAS, CREATE VIEW, and INSERT contexts.
- The expanded pinned audit extracts ordinary Java strings and text blocks. It
  currently accepts 481/484 Trino statements, 231/238 expressions, 68/68 types,
  one Functions statement, five Routines statements, and one standalone function
  specification. It rejects 23/23 direct negative statements and 52/55
  statement error-suite inputs; the retained differences are baseline-approved.
- Known audit differences are identified by content hash in
  `trino_483_audit_baseline.json`. A new mismatch or a smaller extracted corpus
  fails `--fail-on-regression`; PrestoDB remains comparison-only.

## Differential SQLGlot audit

SQLGlot 30.19.0 is pinned only in the `sqlglot-audit` optional dependency. It is
not imported by the package and does not affect `ValidationResult.valid`.
`tools/audit_sqlglot.py` classifies each case as `parsed_ast`,
`command_fallback`, `parse_error`, or `token_error`; treating `Command` as
ordinary parser success would conceal unparsed statement tails.

The current SQLGlot Trino test file was also inventoried at revision
`d01e9461a7a3fcbe50a965c7a2ddf55d41aca97d`. Twenty-seven representative cases
from syntax families absent in this project were adapted into native regression
tests only after Trino 483 verification. Generator-only cross-dialect cases and
permissive `Command` fallbacks are deliberately excluded from positive coverage.

On the stable fixtures SQLGlot produces a full AST for 441/553 prepared positive
statements and errors for 132/236 negative cases; 103 positives and 22 negatives
fall back to `Command`. On Trino 483 positive statements it produces 255 full
ASTs, 199 `Command` fallbacks, and 30 errors. The detailed denominators, corpus
hashes, state counts, and outcome hashes are pinned in
`sqlglot_30_19_0_trino_483_audit_baseline.json`.

An additional native `io.trino:trino-parser:483` run checked 3388 unique
current-valid statements: 3378 parsed, while ten differences reduced to three
fixed false-accept families plus intentional BOM and Trino-master compatibility.
External dollar bodies now require an opening newline; empty `ROLLUP()`/`CUBE()`
and their unquoted scalar-call forms are rejected. Quoted function names and
non-empty grouping-set neighbors remain valid.

These are corpus measurements, not a claim of complete Trino grammar or semantic
coverage. Connector state, names, overload resolution, arity, types, permissions,
table schemas, paths, and execution behavior require a coordinator.

## Parser architecture

The runtime remains Rust-native. `sqlparser-rs` supplies the AST and generic
grammar; `TrinoDialect` adds strict statement handlers and located token
normalizers for Trino productions that are missing upstream. Compatibility
transforms operate on token spans, never regex-rewrite the SQL string, and must
preserve comments, literals, nesting, statement ownership, and diagnostics.

Recognized Trino-only statements that lack an upstream AST may use an internal
placeholder after their complete source shape has been checked. Public statement
metadata never derives its kind from that placeholder: `analyze_statements()`
classifies the original source and reports zero-based indexes and source spans.

The Trino layer includes current catalog/branch/role/privilege/session statements,
nested ALTER operations, SQL routines, CALL and table EXECUTE structure, materialized
view options, table functions, SQL/JSON, MATCH_RECOGNIZE, PIVOT, NEAREST,
CORRESPONDING, inline WITH FUNCTION, WITH SESSION, Iceberg branches/time travel,
and nested ROW/ARRAY/MAP types. Located guards additionally enforce Trino's exact
FETCH, current-value, EXPLAIN, CTE, SQL/JSON, row-pattern, and delimiter shapes.
Negative neighbors are retained for permissive Generic grammar that Trino does
not support.

## Function and type warnings

Function and type checks are advisory name-existence checks. Metadata is collected
from the parsed AST and from located custom-parser records, including wrappers,
SQL/JSON RETURNING/COLUMNS, typed literals, nested ROW fields, ALTER properties,
routine declarations/bodies, CALL arguments, and time-travel expressions.
Synthetic duplicates for the same source span are removed; separate occurrences
remain ordered by source position.

Inline-function exemptions are query-scoped and apply only to unqualified calls.
Procedure names are not scalar functions. SQL embedded in strings, JSON paths,
WKT, external-language dollar bodies, and dynamic SQL is opaque.

The committed catalogs are generated offline from pinned Trino documentation.
`src/functions.manifest.json` and `src/types.manifest.json` record the resolved SHA
and per-file checksums. `--check` is read-only, and missing or malformed required
sources abort before either catalog is replaced. The type catalog includes curated
documented geospatial SQL types without treating Point/Polygon signature labels as
standalone Trino types.

## Safety and templates

Before parsing, inputs are limited to 65,536 significant tokens overall, 4,096 per
statement, and nesting depth 256. Limit failures are ordinary invalid results.

Existing Jinja modes remain supported for compatibility, but template analysis is
a separate workstream in `jinja_dbt_plan_dev.md`. A validator cannot infer macro or
dbt rendering semantics; rendered SQL remains the authoritative input when template
control flow determines SQL shape.
