#![no_main]

use _native::dialects::SqlDialect;
use _native::validate_sql_unchecked;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(sql) = std::str::from_utf8(data) {
        let result = validate_sql_unchecked(sql, &SqlDialect::Trino);
        if result.0 {
            for warning in result.5 {
                assert!(matches!(
                    warning.0.as_str(),
                    "function" | "function_arguments" | "type" | "alias"
                ));
            }
        }
    }
});
