# Fuzz targets

These dev-only targets exercise the Rust core without Python. They require the
separately installed nightly `cargo-fuzz` tool and are not part of wheel/sdist
runtime dependencies.

```bash
cargo +nightly-2026-09-01 fuzz run validation -- -max_total_time=60 -max_len=65536 -rss_limit_mb=1024 -timeout=10
cargo +nightly-2026-09-01 fuzz run analysis -- -max_total_time=60 -max_len=65536 -rss_limit_mb=1024 -timeout=10
cargo +nightly-2026-09-01 fuzz run warnings -- -max_total_time=60 -max_len=65536 -rss_limit_mb=1024 -timeout=10
```

Committed seed corpora in `fuzz/corpus/<target>` cover quoted identifiers,
routines, prepared Jinja-shaped SQL, comments, Unicode, SQL/JSON, table
functions, warning metadata, multiple statements, malformed neighbors, and
nesting. Minimized findings must become normal Rust or pytest regressions before
release.
