# Additional Development Plan: Fixture Coverage

## Analysis Snapshot

The current Python suite passes: `63 passed` with `PYTHONPATH=python pytest -q`.

There are 16 SQL files in `tests/fixtures/`. The test suite directly references
13 of them. Three files are currently not used by pytest:

- `datamart_example.sql` — parses as one valid statement, with one
  `FunctionWarning` for `current_date`.
- `example-queries.sql` — fails at line 65, column 41 on the Trino
  `IPADDRESS '11.255.255.255'` literal.
- `samples.sql` — parses as six valid statements, with no warnings.

The directly tested fixtures have the following status:

- Validated with statement-count assertions: `valid_multi.sql` (3),
  `ddl_multi.sql` (3), `trino_specific.sql` (1), `trino_reports_optimize.sql`
  (17), `trino_iris_queries.sql` (18), `trino_tpch_queries.sql` (39),
  `trino_dbt_customers.sql` (1), `trino_recursive_transformed.sql` (4), and
  `sqlparser_merge_example.sql` (1).
- Intentionally invalid: `invalid_one.sql`.
- Empty input: `empty.sql` (0 statements).
- Expected parser limitations: `trino_reports_tests_schema.sql` fails at
  line 14, column 21 on a nested `row(...)` type; `iceberg_trino_sqldemo.sql`
  fails at line 216, column 28 on `FOR VERSION AS OF` time-travel syntax.

The full fixture scan also found warnings that are not asserted by tests:
`current_timestamp` in `valid_multi.sql`, `current_date` in
`datamart_example.sql`, and `grouping` in `trino_tpch_queries.sql`.

## Documentation Cross-check

The Trino `master` documentation confirms that these are supported Trino
constructs, not semantic-only examples:

- `IPADDRESS '10.0.0.1'` is a documented typed literal in
  `language/types.md`; the failure in `example-queries.sql` is therefore a
  parser-compatibility gap.
- `ROW` fields may contain any SQL type, so nested `row(...)` in
  `trino_reports_tests_schema.sql` is valid Trino syntax and is a parser gap,
  not an invalid fixture.
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
- Added Rust and Python regression tests. The current suite passes with `35`
  Rust tests and `63` Python tests.

Remaining limitation:

- Nested Trino `ROW` data types such as `row(a row(b bigint))` still fail at
  `trino_reports_tests_schema.sql:14`. `sqlparser 0.62` has no dialect hook
  for its data-type parser, so this should be handled as a separate parser
  design task rather than by permissive text masking.

## Planned Work

### 1. Make fixture coverage explicit

- Add a parameterized fixture inventory test for every SQL file.
- Record the expected status, statement count, and, where applicable, expected
  warning names or parser error location.
- Include the three currently untested files so new fixture additions cannot be
  silently ignored.
- Keep input fixtures unchanged unless a fixture is proven malformed by design;
  the test should document the observed contract rather than hide failures.

### 2. Separate supported behavior from accepted limitations

- Design and implement a real nested Trino data-type parser strategy, then
  remove the limitation assertion only after valid nested `ROW`, `ARRAY(ROW)`,
  and `MAP(..., ROW)` cases are covered.
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

1. Add the complete fixture inventory and classify expected outcomes.
2. Build the documentation-derived syntax matrix and classify each unsupported
  form as parser work, accepted limitation, or out of scope.
3. Implement nested row-type parsing with focused Rust and fixture tests.
4. Run the full CI-equivalent verification commands.
