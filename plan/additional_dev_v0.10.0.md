# Additional Development Plan for v0.10.0

## Goal and scope

This plan replaces broad coverage claims with a reproducible, documentation-derived
matrix for Trino SQL syntax. The audit baseline is Trino 483 documentation, checked
on 2026-09-10:

- [SQL statement syntax](https://trino.io/docs/current/sql.html)
- [SELECT](https://trino.io/docs/current/sql/select.html)
- [MATCH_RECOGNIZE](https://trino.io/docs/current/sql/match-recognize.html)
- [PIVOT](https://trino.io/docs/current/sql/pivot.html)
- [JSON functions and JSON_TABLE](https://trino.io/docs/current/functions/json.html)
- [Table functions](https://trino.io/docs/current/functions/table.html)
- [Iceberg connector](https://trino.io/docs/current/connector/iceberg.html)
- [Hive connector](https://trino.io/docs/current/connector/hive.html)
- [HDFS file system support](https://trino.io/docs/current/object-storage/file-system-hdfs.html)

The validator checks syntax and documented function/type names. It does not connect
to Trino, resolve catalogs, inspect connector capabilities, validate table
properties, check function arity or types, or verify HDFS/S3 access. HDFS support is
therefore not a separate SQL grammar: the library can parse Hive or Iceberg SQL that
contains an `hdfs://` location, but it cannot validate `fs.hadoop.enabled`, Hadoop
configuration files, Kerberos, permissions, or storage availability.

## Baseline verification

The v0.9.0 baseline is green:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`: 39 passed
- `pytest -q`: 62 passed
- `ruff check .`
- `mypy python/trino_sql_validator`

All 16 files in `tests/fixtures/` are referenced from pytest. Fifteen have the
expected valid outcome, including the comments-only file; `invalid_one.sql` is
intentionally invalid. There are no completely orphaned fixture files.

| Fixture | Expected result | Statements | Main syntax represented |
| --- | --- | ---: | --- |
| `datamart_example.sql` | valid | 1 | multi-CTE analytics, windows, maps, arrays, filters |
| `ddl_multi.sql` | valid | 3 | Hive-style ORC table, partition property, drop, insert-select |
| `empty.sql` | valid | 0 | comments-only input |
| `example-queries.sql` | valid | 76 | types, date/time, IP, SQL/JSON, arrays, maps, joins, UNNEST |
| `iceberg_trino_sqldemo.sql` | valid with one warning | 100 | Iceberg DDL/DML, metadata tables, branches, time travel, procedures |
| `invalid_one.sql` | invalid | 0 | misspelled `FROM`, line/column reporting |
| `samples.sql` | valid | 6 | Hive/Parquet, geospatial expressions, nested relations |
| `sqlparser_merge_example.sql` | valid | 1 | `MERGE` |
| `trino_dbt_customers.sql` | valid in auto mode | 1 | dbt/Jinja masking and nested CTEs |
| `trino_iris_queries.sql` | valid | 18 | aggregates, maps and window functions |
| `trino_recursive_transformed.sql` | valid | 4 | recursive CTEs nested in derived tables |
| `trino_reports_optimize.sql` | valid | 17 | connector session property and `ALTER TABLE EXECUTE` |
| `trino_reports_tests_schema.sql` | valid | 6 | S3 locations and deeply nested `ROW`/`ARRAY`/`MAP` types |
| `trino_specific.sql` | valid | 1 | `UNNEST ... WITH ORDINALITY` |
| `trino_tpch_queries.sql` | valid | 39 | common query/DDL statements, joins, CTEs, grouping operations |
| `valid_multi.sql` | valid | 3 | basic multi-statement input |

Before the current v0.10.0 work, the only warning in an expected-valid fixture was
`table_changes` at line 118, column 20 of `iceberg_trino_sqldemo.sql`. It is a
documented Iceberg connector table function, so the warning was a false positive
rather than invalid SQL.

## What the current tests do and do not prove

The fixture suite proves that each complete file parses and, for the larger files,
that the parser returns a stable statement count. It does not prove full Trino,
Iceberg, or Hive coverage:

- Most constructs inside large fixtures have no named, isolated regression test.
  Removing or changing one construct can go unnoticed if the file still parses and
  its semicolon count stays unchanged.
- The Iceberg fixture is exempted from the no-warning assertion instead of checking
  the exact expected warning. Additional warning regressions would therefore pass.
- There is no manifest test that fails when a new `.sql` fixture is added without an
  explicit expected result.
- The earlier `85/85` SQL-document claim in `plan/sql-coverage.md` depends on an
  uncommitted `/tmp/opencode/gap_analysis.py` corpus and predates Trino 483. It is not
  reproducible from the repository and must not be treated as current full coverage.
- Connector table properties and procedure arguments are only parsed structurally.
  Their names, combinations and connector-dependent semantics are not checked.
- Negative coverage is narrow. In particular, compatibility rewrites need adjacent
  malformed cases to guard against false acceptance and source-position shifts.
- There is no fixture with an `hdfs://` URI, and no committed official-doc version
  matrix for newer SELECT syntax.

## Trino 483 probe results

The following official or documentation-equivalent examples were run through the
installed v0.9.0 extension. “Supported” means syntax parsing succeeds, not that a
live Trino coordinator would accept the query semantically.

| Area | Syntax | v0.9.0 result | v0.10.0 action |
| --- | --- | --- | --- |
| SELECT | `WITH SESSION` | rejected | add Trino query-prefix parser |
| SELECT | inline `WITH FUNCTION` | rejected | parse UDF declarations and following query |
| SELECT | `MATCH_RECOGNIZE` basic form | supported | retain focused regression |
| SELECT | `MATCH_RECOGNIZE` with official `SUBSET` order | rejected | add compatibility for full clause order |
| SELECT | `PIVOT` without inner `GROUP BY` | supported | retain focused regression |
| SELECT | `PIVOT ... GROUP BY` | rejected | support Trino 483 pivot grouping |
| SELECT | `JSON_TABLE` with nested paths | supported | add fixture regression |
| SELECT | `TABLE(exclude_columns(...))` | valid with false warnings for `TABLE` and `DESCRIPTOR` | classify syntax nodes correctly |
| SELECT/Iceberg | `TABLE(system.table_changes(...))` | valid with false `table_changes` warning | recognize documented connector table function |
| SELECT | `GROUP BY AUTO` | supported | add regression |
| SELECT | `FETCH FIRST ... WITH TIES` | supported | add regression |
| SELECT | `LIMIT ALL` | supported | add regression and remove stale roadmap item |
| SELECT | `TABLESAMPLE BERNOULLI` | supported | add regression |
| SELECT | set operation `CORRESPONDING` | rejected | add parser support |
| SELECT | `CROSS JOIN NEAREST` | rejected | add parser support |
| statement | `EXECUTE IMMEDIATE ... USING` | supported | add regression and remove stale roadmap item |
| statement | `SET TIME ZONE` | supported | add regression |
| statement | `SHOW CREATE FUNCTION` | supported | add regression |
| Iceberg | `FOR VERSION AS OF` numeric or named version | supported | retain regressions |
| Iceberg | `FOR TIMESTAMP AS OF TIMESTAMP/DATE` | rejected | add time-travel compatibility parser |
| Iceberg | materialized view `GRACE PERIOD ... WHEN STALE` | rejected | extend materialized-view parsing |
| Iceberg | `VARIANT` in format v3 table | supported | add regression; semantics remain out of scope |
| Hive | partitioned and bucketed ORC table | supported | add official-example regression |
| Hive | `create_empty_partition` with array arguments | supported | add regression |
| Hive | `drop_stats` with nested arrays | supported | add regression |
| Hive/HDFS | external table with `hdfs://...` string property | supported | add scope-focused regression |
| types | deep `ROW`/`ARRAY`/`MAP` table and cast types | supported | add deeper regression and negative neighbors |

## Development status

Started in the current working tree:

- Added `tests/test_fixture_inventory.py`, which requires an explicit contract for
  every `.sql` fixture and checks exact warnings and intentional errors.
- Added `tests/test_trino_v483_coverage.py` with isolated supported cases and strict
  expected failures for the confirmed Trino 483 parser gaps.
- Added Iceberg `FOR TIMESTAMP AS OF` compatibility for `TIMESTAMP`, `DATE`, and
  general timestamp expressions without shifting source offsets.
- Added token-level validation for materialized-view `GRACE PERIOD`, `WHEN STALE`,
  and `COMMENT` options, including malformed and out-of-order negative cases.
- Added token-level `MATCH_RECOGNIZE SUBSET` validation, including multiple subsets
  and malformed definitions.
- Removed false unknown-function warnings for SQL table-function syntax,
  row-pattern navigation names, and the documented Iceberg `table_changes` function.

After these changes the documentation matrix has five strict expected parser
failures: `WITH SESSION`, inline `WITH FUNCTION`, `PIVOT ... GROUP BY`,
`CORRESPONDING`, and `NEAREST`.

Current verification is green with 46 Rust tests and 110 collected pytest cases
(105 passed and the five documented parser gaps reported as strict expected
failures), plus fmt, clippy with warnings denied, ruff, and mypy.

## Coverage conclusion

The library has broad practical coverage of ordinary Trino queries and the current
fixture corpus, including recursive CTEs, windows, joins, DDL/DML, Iceberg metadata
tables and procedures, and deeply nested structural types. It does not yet cover the
complete Trino 483 grammar. Advanced query prefixes, some current relational
operators, and two important Iceberg clauses are confirmed parser gaps.

Iceberg coverage is substantial but not complete. Hive SQL using HDFS-compatible
locations parses, but “HDFS support” beyond parsing a URI and general Hive/Iceberg
DDL is outside the library's syntax-only contract.

## Implementation plan

### P0 — make the audit reproducible

- [x] Add a fixture inventory test containing the expected valid state, statement count,
  exact warning names, and intentional error for every `.sql` file.
- [x] Add a Trino 483 documentation-derived regression module. Keep supported examples
  green and mark confirmed parser gaps as strict expected failures until implemented.
- [x] Add isolated examples for JSON_TABLE, GROUP BY AUTO, FETCH/LIMIT, TABLESAMPLE,
  EXECUTE IMMEDIATE, Iceberg/Hive procedures, HDFS URI handling, and deeper nested
  structural types.
- [x] Update stale coverage and roadmap claims so supported behavior and planned behavior
  cannot be confused.

### P1 — high-value correctness gaps

- [x] Support Iceberg `FOR TIMESTAMP AS OF` with `TIMESTAMP` and `DATE` expressions while
  preserving source offsets.
- [x] Support `GRACE PERIOD` and `WHEN STALE` in `CREATE MATERIALIZED VIEW`.
- [x] Stop reporting `TABLE` and `DESCRIPTOR` syntax as unknown functions.
- [x] Recognize the documented Iceberg `table_changes` table function without implying
  that arbitrary connector or plugin functions are globally known.
- [x] Add malformed neighboring cases for the new v0.10.0 compatibility paths.

### P2 — current SELECT grammar

- Support `WITH SESSION`, including qualified catalog properties and multiple values.
- Support one or more inline `WITH FUNCTION` declarations, with type and called-body
  warning traversal where the AST permits it.
- [x] Complete official `MATCH_RECOGNIZE` clause ordering, including `SUBSET`.
- Support Trino 483 `PIVOT ... GROUP BY`, `CORRESPONDING`, and `NEAREST`.
- Add row-pattern window tests separately from `MATCH_RECOGNIZE`.

### P3 — corpus depth and safety

- Add isolated tests for statement families that currently occur only inside large
  files: branches, metadata tables, `ALTER TABLE EXECUTE`, `CALL`, `PREPARE`, `MERGE`,
  and `UNNEST`.
- Add UTF-8, BOM, CRLF, multiline warning/error position, and Jinja interaction cases.
- Test that normalization keywords inside strings, quoted identifiers, and comments
  are never rewritten as SQL syntax.
- Add deeper nested type combinations and malformed/unbalanced variants in Rust, with
  public API counterparts in pytest.

### P4 — release gate

- [x] Record the exact Trino documentation version used by the syntax audit and cases
  (Trino 483). Record the source revision separately when generated catalogs change.
- Re-run every fixture and the documentation matrix after upgrading `sqlparser`.
- [x] Require `fmt`, clippy with warnings denied, Rust tests, pytest, ruff, and mypy to be
  green with no unexpected passes.
- [x] Update `CHANGELOG.md`, `plan/roadmap.md`, and version files for the implemented
  v0.10.0 scope. Create and push the release tag only with explicit user authorization;
  authorization was provided for this release.

## Definition of done

v0.10.0 is ready when the repository contains a reproducible fixture and Trino 483
coverage matrix, the P1 gaps are implemented with positive and negative tests, all
remaining unsupported P2/P3 syntax is explicitly tracked, exact expected warnings are
asserted, and the full CI command set passes.
