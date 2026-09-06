# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-09-06

Initial release.

### Added

- `validate(sql, dialect=...)` — validate a string containing one or more Trino SQL
  statements.
- `validate_file(path, dialect=...)` — validate a UTF-8 `.sql` file.
- `ValidationResult` / `Error` result objects: invalid SQL is a value, not an
  exception; errors report message + line/column.
- Dialects: `trino` (custom `TrinoDialect` refusing backquoted identifiers),
  `hive`, `generic`.
- Rust core via PyO3 + `sqlparser-rs`: ABI3 wheel for Python >= 3.10.
- CI (`.github/workflows/ci.yml`): fmt, clippy, Rust + Python tests, ruff, mypy.
- Release pipeline (`.github/workflows/release.yml`): wheels for Linux
  (x86_64/aarch64 manylinux), macOS (arm64/x86_64), Windows (x86_64) + sdist,
  published to PyPI via Trusted Publishing on tag `v*`.