# Additional Development Plan: Fixture Coverage

## Analysis Snapshot

The current suites pass with `39` Rust tests and `62` Python tests.

All 16 SQL files in `tests/fixtures/` are referenced by pytest. Fourteen parse
as valid SQL, `empty.sql` is valid with zero statements, and `invalid_one.sql`
is intentionally invalid. The larger fixture counts are asserted explicitly:
`example-queries.sql` (76), `iceberg_trino_sqldemo.sql` (100),
`trino_reports_tests_schema.sql` (6), `trino_tpch_queries.sql` (39),
`trino_iris_queries.sql` (18), and `trino_reports_optimize.sql` (17).

The fixture corpus produces no warnings except the expected non-fatal
`table_changes` plugin-function warning in `iceberg_trino_sqldemo.sql`.

## Documentation Cross-check

The Trino `master` documentation confirms that these are supported Trino
constructs, not semantic-only examples:

- `IPADDRESS '10.0.0.1'` is a documented typed literal in
  `language/types.md`; the failure in `example-queries.sql` is therefore a
  parser-compatibility gap.
- `ROW` fields may contain any SQL type, so nested `row(...)` in
  `trino_reports_tests_schema.sql` is valid Trino syntax and required a parser
  compatibility layer rather than an invalid-fixture exception.
- Iceberg documents `FOR VERSION AS OF` time travel, named branch/tag
  references, and `ALTER TABLE ... EXECUTE` procedures. The failure in
  `iceberg_trino_sqldemo.sql` is therefore a missing grammar feature, while
  the branch and execute statements that already parse need targeted tests.
- `current_date`, `current_timestamp`, `localtime`, and `localtimestamp` are
  documented SQL-standard functions without parentheses. They must not be
  reported as unknown function calls.
- `grouping(...)` is documented in the `GROUPING operation` section, and
  `histogram(...)` is documented in aggregate functions. Warnings for these
  names indicate catalog extraction or AST classification drift.

The current documentation also exposes syntax not represented by the existing
fixture plan: `WITH SESSION`, inline `WITH FUNCTION`, `MATCH_RECOGNIZE`,
`PIVOT`, `TABLE(...)` table functions, `JSON_TABLE`, `NEAREST`, `FETCH FIRST`,
`TABLESAMPLE`, and `CORRESPONDING` set operations. These should be added to a
documentation-derived coverage matrix rather than inferred only from current
fixtures.

## Implementation Status

Completed in the current implementation:

- Added regression coverage for the previously untested valid fixtures.
- Added Trino normalization for `IPADDRESS '...'`, Iceberg `FOR VERSION AS OF`
  (including named references), Iceberg branch references, `VALUES VARCHAR`,
  `VALUES ARRAY[...]`, `VALUES map_from_entries(...)`, and `ALTER TABLE
  EXECUTE ... WHERE`.
- Enabled `sqlparser` table-version parsing for the Trino dialect.
- Synchronized the generated function catalog with the Trino docs and added
  special handling for no-parentheses date/time expressions, `grouping`, and
  JSON functions.
- Added token-level parsing compatibility for nested Trino `ROW` types and
  their `ARRAY`/`MAP` containers while preserving source spans and ordinary
  `ROW(...)` value constructors.
- Added Rust and Python regression tests. The current suites pass with `39`
  Rust tests and `62` Python tests, and every expected-valid fixture parses.

## Planned Work

### 1. Consolidate fixture coverage

- Consolidate the existing direct fixture tests into a parameterized inventory
  so new SQL files cannot be added without an explicit expected result.
- Record the expected status, statement count, and, where applicable, expected
  warning names or parser error location.
- Keep input fixtures unchanged unless a fixture is proven malformed by design;
  the test should document the observed contract rather than hide failures.

### 2. Separate supported behavior from accepted limitations

- Keep focused regressions for nested `ROW`, `ARRAY(ROW)`, and
  `MAP(..., ROW)` types, source positions, and `ROW(...)` value constructors.
- Keep parser regressions for `IPADDRESS`, Iceberg time travel, branch
  references, and Iceberg `ALTER TABLE EXECUTE` forms.

### 3. Correct catalog-warning accuracy

- Exclude the documented no-parentheses date/time expressions (`current_date`,
  `current_time`, `current_timestamp`, `localtime`, and `localtimestamp`) from
  unknown-function warnings, or represent them as non-call expressions in the
  AST walk.
- Add the documented `grouping` operation to the generated catalog or special
  handling, and verify that `histogram` remains recognized as an aggregate
  function.
- Compare the generated function/type catalogs with the current Trino source
  docs during regeneration; record the documentation revision used for each
  generated catalog because `master` can evolve independently of the pinned
  parser release.
- Add assertions for the intended warning behavior in the affected fixtures,
  including warning type, name, and source position.
- Keep warning checks non-fatal to `valid`, consistent with the documented
  catalog design.

### 4. Expand structural coverage by SQL feature

- Use the existing fixtures to add focused assertions for statements currently
  only covered by aggregate file-level success: `SHOW`, `DESCRIBE`, `EXPLAIN`,
  `CALL`, `PREPARE`, `UNNEST`, `MERGE`, Iceberg branches, and `ALTER TABLE
  EXECUTE`.
- Add a documentation-derived syntax matrix for the uncovered `SELECT` forms:
  `WITH SESSION`, inline `WITH FUNCTION`, `MATCH_RECOGNIZE`, `PIVOT`, table
  functions, `JSON_TABLE`, `NEAREST`, `FETCH FIRST`, `TABLESAMPLE`, and
  `CORRESPONDING`.
- Add negative cases for malformed forms adjacent to the custom Trino parser
  overrides, especially missing operands and unbalanced groups.
- Verify that multi-statement validation reports the intended count and that a
  parse failure does not produce catalog warnings.

### 5. Add environment-independent regression cases

- Cover UTF-8 text, BOM/CRLF input, line/column preservation, and path-like
  arguments for `validate_file`.
- Keep the real extension in the loop for Python integration tests; place pure
  parser behavior in Rust tests according to the repository convention.
- Run `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D
  warnings`, `pytest -q`, `ruff check .`, and mypy after implementation.

## Suggested Order

1. Consolidate the complete fixture inventory and classify expected outcomes.
2. Build the documentation-derived syntax matrix and classify each unsupported
  form as parser work, accepted limitation, or out of scope.
3. Add the environment-independent regression cases.
4. Run the full CI-equivalent verification commands.
