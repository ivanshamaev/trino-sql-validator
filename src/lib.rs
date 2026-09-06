use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::str::FromStr;
use std::sync::LazyLock;

use crate::dialects::SqlDialect;
use sqlparser::parser::{Parser, ParserError};

pub mod dialects;

const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

type ValidationResultTuple = (bool, usize, Option<String>, Option<usize>, Option<usize>);

static LOCATION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"at Line: (\d+), Column: (\d+)").unwrap());

static LOCATION_SUFFIX_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s+at Line: \d+, Column: \d+$").unwrap());

pub fn validate_sql_impl(sql: &str, dialect: &SqlDialect) -> ValidationResultTuple {
    let parser = dialect.parser();
    match Parser::parse_sql(parser.as_ref(), sql) {
        Ok(statements) => (true, statements.len(), None, None, None),
        Err(err) => {
            let (message, line, column) = error_details(err);
            (false, 0, Some(message), line, column)
        }
    }
}

fn error_details(err: ParserError) -> (String, Option<usize>, Option<usize>) {
    let full = err.to_string();
    let (line, column) = extract_location(&full);
    let message = strip_location(&full);
    (message, line, column)
}

fn strip_location(message: &str) -> String {
    // sqlparser appends " at Line: N, Column: M" to many errors; we report the
    // location separately, so remove the redundant suffix.
    LOCATION_SUFFIX_PATTERN.replace(message, "").to_string()
}

fn extract_location(message: &str) -> (Option<usize>, Option<usize>) {
    // sqlparser error strings embed location as "at Line: N, Column: M"
    match LOCATION_PATTERN.captures(message) {
        Some(captures) => (
            captures.get(1).and_then(|m| m.as_str().parse().ok()),
            captures.get(2).and_then(|m| m.as_str().parse().ok()),
        ),
        None => (None, None),
    }
}

/// Validate a SQL string (one or more statements) against a dialect.
///
/// Returns `(valid, statement_count, error_message, error_line, error_column)`.
///
/// Invalid SQL is reported as a tuple value — this function never raises for
/// bad syntax. Only real programming errors (e.g. unknown dialect) raise.
#[pyfunction]
#[pyo3(signature = (sql, dialect = "trino"))]
fn validate(sql: &str, dialect: &str) -> PyResult<ValidationResultTuple> {
    let parsed_dialect = SqlDialect::from_str(dialect)?;
    Ok(validate_sql_impl(sql, &parsed_dialect))
}

/// Validate a UTF-8 file containing one or more statements.
///
/// Returns the same tuple shape as [`validate`]. File-level errors (missing file,
/// decode failure) raise an exception.
#[pyfunction]
#[pyo3(signature = (path, dialect = "trino"))]
fn validate_file(path: &str, dialect: &str) -> PyResult<ValidationResultTuple> {
    let parsed_dialect = SqlDialect::from_str(dialect)?;
    let contents = std::fs::read_to_string(path).map_err(|err| {
        PyValueError::new_err(format!("failed to read SQL file '{}': {}", path, err))
    })?;
    Ok(validate_sql_impl(&contents, &parsed_dialect))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", PACKAGE_VERSION)?;
    m.add("__doc__", "Rust-native core for trino_sql_validator.")?;
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(validate_file, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trino() -> SqlDialect {
        SqlDialect::Trino
    }

    #[test]
    fn empty_sql_is_valid_zero_statements() {
        let (valid, count, _, _, _) = validate_sql_impl("", &trino());
        assert!(valid);
        assert_eq!(count, 0);
    }

    #[test]
    fn comments_only_is_zero_statements() {
        let (valid, count, _, _, _) = validate_sql_impl("-- hello\n/* block */", &trino());
        assert!(valid);
        assert_eq!(count, 0);
    }

    #[test]
    fn single_statement_is_valid() {
        let (valid, count, _, _, _) = validate_sql_impl("SELECT 1", &trino());
        assert!(valid);
        assert_eq!(count, 1);
    }

    #[test]
    fn multiple_statements_are_valid() {
        let (valid, count, _, _, _) = validate_sql_impl(
            "SELECT 1; SELECT * FROM t WHERE a > 0; DROP TABLE x;",
            &trino(),
        );
        assert!(valid);
        assert_eq!(count, 3);
    }

    #[test]
    fn trailing_semicolon_is_fine() {
        let (valid, count, _, _, _) = validate_sql_impl("SELECT 1;", &trino());
        assert!(valid);
        assert_eq!(count, 1);
    }

    #[test]
    fn invalid_sql_reports_location() {
        let (valid, count, message, line, column) = validate_sql_impl("SELECT * FORM", &trino());
        assert!(!valid);
        assert_eq!(count, 0);
        let message = message.unwrap();
        assert!(message.contains("Expected"));
        assert_eq!(line, Some(1));
        assert!(column.is_some());
    }

    #[test]
    fn trino_accepts_string_backslash_escape() {
        let (valid, _, _, _, _) = validate_sql_impl("SELECT 'ab\\'cd'", &trino());
        assert!(valid);
    }

    #[test]
    fn unknown_dialect_raises() {
        assert!(SqlDialect::from_str("mysql").is_err());
    }
}
