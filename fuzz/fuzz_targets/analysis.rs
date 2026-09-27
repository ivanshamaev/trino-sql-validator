#![no_main]

use _native::analyze_sql_impl;
use _native::dialects::SqlDialect;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(sql) = std::str::from_utf8(data) {
        let _ = analyze_sql_impl(sql, &SqlDialect::Trino);
    }
});
