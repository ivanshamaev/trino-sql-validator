use core::ops::ControlFlow;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use sqlparser::ast::visit_expressions;
use sqlparser::ast::{Expr, Ident, Statement};
use std::str::FromStr;
use std::sync::LazyLock;

use crate::dialects::SqlDialect;
use sqlparser::parser::{Parser, ParserError};

pub mod dialects;
pub mod functions;

const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `(valid, statement_count, error_message, error_line, error_column,
/// warnings)` where each warning is `(function_name, line, column)`.
type ValidationResultTuple = (
    bool,
    usize,
    Option<String>,
    Option<usize>,
    Option<usize>,
    Vec<(String, Option<usize>, Option<usize>)>,
);

static LOCATION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"at Line: (\d+), Column: (\d+)").unwrap());

static LOCATION_SUFFIX_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s+at Line: \d+, Column: \d+$").unwrap());

pub fn validate_sql_impl(sql: &str, dialect: &SqlDialect) -> ValidationResultTuple {
    let parser = dialect.parser();
    match Parser::parse_sql(parser.as_ref(), sql) {
        Ok(statements) => {
            let warnings = if *dialect == SqlDialect::Trino {
                find_unknown_functions(&statements)
            } else {
                Vec::new()
            };
            (true, statements.len(), None, None, None, warnings)
        }
        Err(err) => {
            let (message, line, column) = error_details(err);
            (false, 0, Some(message), line, column, Vec::new())
        }
    }
}

/// Walk every expression in the parsed statements and collect function calls
/// whose name is not in the Trino catalog, with the call site's line/column.
fn find_unknown_functions(
    statements: &Vec<Statement>,
) -> Vec<(String, Option<usize>, Option<usize>)> {
    let mut unknown = Vec::new();
    let _ = visit_expressions(statements, |expr| {
        if let Expr::Function(func) = expr {
            if let Some(ident) = func.name.0.last().and_then(|part| part.as_ident()) {
                let name = ident.value.to_ascii_lowercase();
                if !functions::is_known_function(&name) {
                    let (line, column) = span_position(ident);
                    unknown.push((name, line, column));
                }
            }
        }
        ControlFlow::<()>::Continue(())
    });
    unknown
}

fn span_position(ident: &Ident) -> (Option<usize>, Option<usize>) {
    let start = ident.span.start;
    if start.line == 0 {
        (None, None)
    } else {
        (Some(start.line as usize), Some(start.column as usize))
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
/// Returns `(valid, statement_count, error_message, error_line, error_column,
/// warnings)`. For the `trino` dialect, `warnings` reports calls to functions
/// that are not in the documented Trino catalog (each entry is
/// `(name, line, column)`).
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
        let (valid, count, _, _, _, _) = validate_sql_impl("", &trino());
        assert!(valid);
        assert_eq!(count, 0);
    }

    #[test]
    fn comments_only_is_zero_statements() {
        let (valid, count, _, _, _, _) = validate_sql_impl("-- hello\n/* block */", &trino());
        assert!(valid);
        assert_eq!(count, 0);
    }

    #[test]
    fn single_statement_is_valid() {
        let (valid, count, _, _, _, _) = validate_sql_impl("SELECT 1", &trino());
        assert!(valid);
        assert_eq!(count, 1);
    }

    #[test]
    fn multiple_statements_are_valid() {
        let (valid, count, _, _, _, _) = validate_sql_impl(
            "SELECT 1; SELECT * FROM t WHERE a > 0; DROP TABLE x;",
            &trino(),
        );
        assert!(valid);
        assert_eq!(count, 3);
    }

    #[test]
    fn trailing_semicolon_is_fine() {
        let (valid, count, _, _, _, _) = validate_sql_impl("SELECT 1;", &trino());
        assert!(valid);
        assert_eq!(count, 1);
    }

    #[test]
    fn invalid_sql_reports_location() {
        let (valid, count, message, line, column, _) = validate_sql_impl("SELECT * FORM", &trino());
        assert!(!valid);
        assert_eq!(count, 0);
        let message = message.unwrap();
        assert!(message.contains("Expected"));
        assert_eq!(line, Some(1));
        assert!(column.is_some());
    }

    #[test]
    fn trino_accepts_string_backslash_escape() {
        let (valid, _, _, _, _, _) = validate_sql_impl("SELECT 'ab\\'cd'", &trino());
        assert!(valid);
    }

    #[test]
    fn unknown_dialect_raises() {
        assert!(SqlDialect::from_str("mysql").is_err());
    }

    #[test]
    fn known_functions_produce_no_warnings() {
        let (_, _, _, _, _, warnings) =
            validate_sql_impl("SELECT round(1.5), array_agg(x), count(*) FROM t", &trino());
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_function_reports_name_and_position() {
        let (valid, _, _, _, _, warnings) = validate_sql_impl("SELECT marh(1.5)", &trino());
        assert!(valid);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "marh");
        assert_eq!(warnings[0].1, Some(1));
        assert_eq!(warnings[0].2, Some(8)); // "marh(" starts at column 8
    }

    #[test]
    fn nested_and_qualified_calls_are_found() {
        let (_, _, _, _, _, warnings) = validate_sql_impl(
            "SELECT round(marh(x)), schema.foobar(y), baz FROM t",
            &trino(),
        );
        let names: Vec<_> = warnings.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, vec!["marh", "foobar"]);
    }

    #[test]
    fn unknown_function_position_on_second_line() {
        let (_, _, _, _, _, warnings) =
            validate_sql_impl("SELECT 1\nFROM t\nWHERE x = zort(2)", &trino());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "zort");
        assert_eq!(warnings[0].1, Some(3));
        assert_eq!(warnings[0].2, Some(11)); // "zort" in "WHERE x = zort(2)"
    }

    #[test]
    fn function_checking_only_applies_to_trino() {
        let (_, _, _, _, _, warnings) = validate_sql_impl("SELECT marh(1)", &SqlDialect::Generic);
        assert!(warnings.is_empty());
    }

    #[test]
    fn invalid_sql_produces_no_warnings() {
        let (valid, _, _, _, _, warnings) = validate_sql_impl("SELECT marh(1 FORM", &trino());
        assert!(!valid);
        assert!(warnings.is_empty());
    }
}
