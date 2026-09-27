# Trino SQL coverage and validation boundaries

Status: v0.21.0, Trino 483 pin, SQLGlot 30.19.0 comparison pin.

## Measured coverage

- All 78 SQL files in `tests/fixtures` have explicit outcome, statement-count,
  warning, and feature-profile expectations. All 553 statements from positive
  non-empty fixtures are also validated independently.
- The 30 files in `tests/fixtures/invalid_datamarts` are diagnostic fixtures:
  29 parser-valid files return exact catalog warnings; the unsupported
  `percentile_cont(...) WITHIN GROUP` form is a located parser error. They are
  intentionally excluded from the positive corpus.
- The current automated suite collects 5,015 pytest cases and 87 Rust unit tests.
- `trino_invalid_sql.sql` contributes 236 independent parser-negative cases;
  each is passed to `validate()` separately rather than as multi-statement SQL.
  Trino 483 and v0.21.0 both reject all 236, compared with 201/236 rejections in
  v0.17.0.
- The existing 57-case composition matrix is supplemented by an 84-cell,
  native-verified family/polarity/wrapper matrix, a 100-case grammar corpus,
  a 28-cell LF/CRLF transformation matrix, and six native-labelled token-deletion
  mutations.
- The expanded pinned audit extracts ordinary Java strings and text blocks. It
  currently accepts 484/484 Trino statements, 238/238 expressions, 68/68 types,
  one Functions statement, five Routines statements, and one standalone function
  specification. It rejects 23/23 direct negative statements and 52/55
  statement error-suite inputs; the retained differences are baseline-approved.
- Audit sections pin exact corpus and upstream-source hashes. A new mismatch,
  changed or missing case, changed wrapper, or stale allowlist entry fails
  `--fail-on-regression`; PrestoDB remains comparison-only.
- The protocol-v2 native parser oracle covers 1,040 inputs. Its last full run
  produced 710 `accepted`, 330 `rejected`, zero infrastructure errors, and zero
  expected-label mismatches. It records raw and prepared hashes per input plus
  Java, harness, classpath-file, class, and dependency-JAR provenance.
- The v0.21.0 focused `::` corpus adds nine rejected cast-like forms and six
  accepted static-method controls, independently checked with zero expected-label
  mismatches by `io.trino:trino-parser:483` on Temurin JRE 25.0.4.1.

## Compatibility and provenance matrix

| Component | Pinned identity | Contract |
| --- | --- | --- |
| Library/runtime | `trino-sql-validator 0.21.0`; Rust stable; Python ABI3 >= 3.10 | Validation is Rust-native; Java, Maven, ANTLR, SQLGlot, and subprocess parsers are absent from runtime and distributions. |
| Trino source | release `483`, revision `50b0b50b75abd47f830b7805ee1b51716eb4065e` | Parser cases, grammar, docs catalogs, and the native oracle use the same release pin. |
| `SqlBase.g4` | `dd7f545bd88187969453cd1544747ffcc0f788d02b38ce1f9da0c5e35638d184` | Inventory covers all 144 parser rules and 708 top-level alternatives; 48 rules are partial, six entry-point-only, and 90 remain explicit backlog. |
| `SqlParser.java` / `PostProcessor` | `3172e1f7e0df1feb6d4310d819a9fb48bb9b8d86eb969743161b6b759cb2c90c` | Lexer/post-processing parity is tracked separately from grammar text and is only partially covered. |
| `AstBuilder.java` | `987bd948349f6f951d60ef8aebf8e7281e217194eafff1658646e3653f7cd541` | AST-building behavior is fingerprinted and explicitly marked partial. |
| Java oracle | `io.trino:trino-parser:483`, JDK 25, protocol schema 2 | The 1,040-case report hashes the Maven descriptor, harness source, compiled classes, classpath file, every JAR, and the Java runtime. Infrastructure failures never become SQL rejections. |
| Function/type catalogs | Trino revision above; 472 functions and 39 types | `src/functions.manifest.json` and `src/types.manifest.json` pin every documentation input hash. Checks are advisory name existence only. |
| Parser audit | `plan/trino_483_audit_baseline.json`; exact source hashes per upstream suite | Frozen 484 statement, 238 expression, 68 type, and negative-suite denominators remain distinct from new entry-point sections. |
| SQLGlot audit | SQLGlot `30.19.0`, same Trino revision and source hashes | Offline comparison only; `Command` fallback is not treated as a full parse. |
| Intentional compatibility | one leading UTF-8 BOM; two `ALTER MATERIALIZED VIEW ... EXECUTE` forms from newer Trino | These three cases are explicit native-483 rejections accepted by the library; they are not counted as parser fidelity. |

The grammar inventory is deliberately conservative: 321/708 alternatives have
selected focused evidence and 387 remain `missing`. A rule marked `partial`
means only that named alternatives or interactions are exercised, not that all
optional clauses, recursive paths, lexer actions, post-processing, or AST
construction are covered.

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

The 30 diagnostic data marts are tracked in a separate SQLGlot section:
29 produce a full AST and one produces a parse error. This classification is
not engine validity and does not make SQLGlot a runtime backend.

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

The focused inventory records all 144 Trino 483 parser rules and 708 top-level
alternatives as partial, entry-point-only, or missing coverage. It fingerprints
the grammar, `SqlParser`/`PostProcessor`, and `AstBuilder` separately. The Java
`SqlParser` oracle is an explicit dev/audit tool only; default tests, wheels,
imports, and validation never start a JVM. The production core also builds as
an `rlib` without the optional Python/PyO3 feature for unit and fuzz targets.

Three libFuzzer targets exercise uncaught validation internals, statement
analysis, and warning extraction. The committed 18-file seed corpus is checked
by normal tests; a bounded local smoke completed 26,626, 23,395, and 21,328
iterations respectively without crash or timeout. Scheduled CI repeats each
target for 60 seconds with a 64-KiB input cap, 1-GiB RSS cap, and 10-second
per-input timeout, and uploads corpus/findings.

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
