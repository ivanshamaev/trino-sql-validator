# Trino SQL coverage and validation boundaries

Status: v0.14.0 development baseline, Trino 483 pin.

## Measured coverage

- All 16 files in `tests/fixtures` have explicit outcome, statement-count,
  warning, and feature-profile expectations. All 276 statements from positive
  non-empty fixtures are also validated independently.
- The 57-case composition matrix exercises query features under their supported
  root, EXPLAIN, PREPARE, CTAS, CREATE VIEW, and INSERT contexts.
- The expanded pinned audit extracts ordinary Java strings and text blocks. It
  currently accepts 476/484 Trino statements, 231/238 expressions, 68/68 types,
  one Functions statement, five Routines statements, and one standalone function
  specification. It rejects 23/23 direct negative statements.
- Known audit differences are identified by content hash in
  `trino_483_audit_baseline.json`. A new mismatch or a smaller extracted corpus
  fails `--fail-on-regression`; PrestoDB remains comparison-only.

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
and nested ROW/ARRAY/MAP types. Negative neighbors are retained for permissive
Generic grammar that Trino does not support.

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
