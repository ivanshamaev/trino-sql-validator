use core::ops::ControlFlow;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use sqlparser::ast::visit_expressions;
use sqlparser::ast::{ArrayElemTypeDef, DataType, Expr, Ident, ObjectNamePart, Statement};
use std::str::FromStr;
use std::sync::LazyLock;

use crate::dialects::SqlDialect;
use sqlparser::parser::{Parser, ParserError};

pub mod dialects;
pub mod functions;
pub mod types;

const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `(valid, statement_count, error_message, error_line, error_column,
/// warnings)` where each warning is `(kind, name, line, column)` and `kind`
/// is `"function"` or `"type"`.
type ValidationResultTuple = (
    bool,
    usize,
    Option<String>,
    Option<usize>,
    Option<usize>,
    Vec<(String, String, Option<usize>, Option<usize>)>,
);

static LOCATION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"at Line: (\d+), Column: (\d+)").unwrap());

static LOCATION_SUFFIX_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s+at Line: \d+, Column: \d+$").unwrap());

/// Trino writes prepared statements as `PREPARE name FROM <query>`; sqlparser
/// expects `PREPARE name AS <query>`. Normalize the first `FROM` of a PREPARE
/// statement (there is nothing syntactically between `PREPARE <name>` and the
/// keyword, so a regex is safe here).
static PREPARE_FROM_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)\b(PREPARE\s+[a-zA-Z_][a-zA-Z0-9_$]*)\s+FROM\b").unwrap()
});

fn normalize_prepare_from(sql: &str) -> String {
    PREPARE_FROM_PATTERN
        .replace_all(sql, |captures: &regex::Captures<'_>| {
            let matched = captures.get(0).unwrap().as_str();
            let prefix = captures.get(1).unwrap().as_str();
            format!(
                "{prefix} AS{}",
                " ".repeat(matched.len() - prefix.len() - 3)
            )
        })
        .into()
}

pub fn validate_sql_impl(sql: &str, dialect: &SqlDialect) -> ValidationResultTuple {
    let parser = dialect.parser();
    let sql = if *dialect == SqlDialect::Trino {
        normalize_prepare_from(sql)
    } else {
        sql.to_string()
    };
    match Parser::parse_sql(parser.as_ref(), &sql) {
        Ok(statements) => {
            let mut warnings = if *dialect == SqlDialect::Trino {
                let mut warnings = find_unknown_functions(&statements);
                find_unknown_types(&statements, &mut warnings);
                warnings
            } else {
                Vec::new()
            };
            warnings.sort_by_key(|w| (w.2.unwrap_or(usize::MAX), w.3.unwrap_or(usize::MAX)));
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
) -> Vec<(String, String, Option<usize>, Option<usize>)> {
    let mut unknown = Vec::new();
    let _ = visit_expressions(statements, |expr| {
        if let Expr::Function(func) = expr {
            if let Some(ident) = func.name.0.last().and_then(|part| part.as_ident()) {
                let name = ident.value.to_ascii_lowercase();
                if !functions::is_known_function(&name) {
                    let (line, column) = span_position(ident);
                    unknown.push(("function".to_string(), name, line, column));
                }
            }
        }
        ControlFlow::<()>::Continue(())
    });
    unknown
}

/// Collect data types that are not in the Trino catalog into `warnings`,
/// tagged with kind `"type"` and the type's line/column. Casts are found via
/// the expression walker; statement-level type declarations (table columns,
/// view columns, `ALTER TABLE` column operations, function return types) are
/// visited directly.
fn find_unknown_types(
    statements: &Vec<Statement>,
    warnings: &mut Vec<(String, String, Option<usize>, Option<usize>)>,
) {
    for statement in statements {
        match statement {
            Statement::CreateTable(create) => {
                for column in &create.columns {
                    collect_type_entries(&column.data_type, warnings);
                }
            }
            Statement::CreateView(create) => {
                for column in &create.columns {
                    if let Some(data_type) = &column.data_type {
                        collect_type_entries(data_type, warnings);
                    }
                }
            }
            Statement::AlterTable(alter) => {
                for operation in &alter.operations {
                    match operation {
                        sqlparser::ast::AlterTableOperation::AddColumn { column_def, .. } => {
                            collect_type_entries(&column_def.data_type, warnings);
                        }
                        sqlparser::ast::AlterTableOperation::ChangeColumn { data_type, .. }
                        | sqlparser::ast::AlterTableOperation::ModifyColumn { data_type, .. } => {
                            collect_type_entries(data_type, warnings);
                        }
                        sqlparser::ast::AlterTableOperation::AlterColumn {
                            op: sqlparser::ast::AlterColumnOperation::SetDataType { data_type, .. },
                            ..
                        } => {
                            collect_type_entries(data_type, warnings);
                        }
                        sqlparser::ast::AlterTableOperation::AlterColumn { .. } => {}
                        _ => {}
                    }
                }
            }
            Statement::CreateFunction(func) => {
                if let Some(return_type) = &func.return_type {
                    match return_type {
                        sqlparser::ast::FunctionReturnType::DataType(data_type)
                        | sqlparser::ast::FunctionReturnType::SetOf(data_type) => {
                            collect_type_entries(data_type, warnings);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let _ = visit_expressions(statements, |expr| {
        if let Expr::Cast { data_type, .. } = expr {
            collect_type_entries(data_type, warnings);
        }
        ControlFlow::<()>::Continue(())
    });
}

fn collect_type_entries(
    data_type: &DataType,
    warnings: &mut Vec<(String, String, Option<usize>, Option<usize>)>,
) {
    match data_type {
        DataType::Array(elem_type) => match elem_type {
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::Parenthesis(inner)
            | ArrayElemTypeDef::SquareBracket(inner, _) => {
                collect_type_entries(inner, warnings);
            }
            ArrayElemTypeDef::None => {}
        },
        DataType::Struct(fields, _) => {
            for field in fields {
                collect_type_entries(&field.field_type, warnings);
            }
        }
        DataType::Map(key_type, value_type) => {
            collect_type_entries(key_type, warnings);
            collect_type_entries(value_type, warnings);
        }
        DataType::Custom(name, _) => {
            let Some(ObjectNamePart::Identifier(ident)) = name.0.last() else {
                return;
            };
            let type_name = ident.value.to_ascii_lowercase();
            if !types::is_known_type(&type_name) {
                let (line, column) = span_position(ident);
                warnings.push(("type".to_string(), type_name, line, column));
            }
        }
        _ => {}
    }
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
/// and uses of data types that are not in the documented Trino catalog (each
/// entry is `(kind, name, line, column)` where `kind` is `"function"` or
/// `"type"`).
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
    fn trino_treats_backslash_as_string_content() {
        let (valid, _, _, _, _, _) = validate_sql_impl("SELECT 'ab\\\\cd'", &trino());
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
        assert_eq!(warnings[0].0, "function");
        assert_eq!(warnings[0].1, "marh");
        assert_eq!(warnings[0].2, Some(1));
        assert_eq!(warnings[0].3, Some(8)); // "marh(" starts at column 8
    }

    #[test]
    fn nested_and_qualified_calls_are_found() {
        let (_, _, _, _, _, warnings) = validate_sql_impl(
            "SELECT round(marh(x)), schema.foobar(y), baz FROM t",
            &trino(),
        );
        let names: Vec<_> = warnings
            .iter()
            .filter(|w| w.0 == "function")
            .map(|w| w.1.as_str())
            .collect();
        assert_eq!(names, vec!["marh", "foobar"]);
    }

    #[test]
    fn unknown_function_position_on_second_line() {
        let (_, _, _, _, _, warnings) =
            validate_sql_impl("SELECT 1\nFROM t\nWHERE x = zort(2)", &trino());
        let warnings: Vec<_> = warnings.into_iter().filter(|w| w.0 == "function").collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].1, "zort");
        assert_eq!(warnings[0].2, Some(3));
        assert_eq!(warnings[0].3, Some(11)); // "zort" in "WHERE x = zort(2)"
    }

    #[test]
    fn known_types_produce_no_warnings() {
        let (_, _, _, _, _, warnings) = validate_sql_impl(
            "CREATE TABLE t (a bigint, b varchar, c decimal(10,2), d row(x integer))",
            &trino(),
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_type_in_column_def_reports_name_and_position() {
        let (valid, _, _, _, _, warnings) =
            validate_sql_impl("CREATE TABLE t (a bignum)", &trino());
        assert!(valid);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "type");
        assert_eq!(warnings[0].1, "bignum");
        assert_eq!(warnings[0].2, Some(1));
        assert_eq!(warnings[0].3, Some(19)); // "bignum" in "(a bignum)"
    }

    #[test]
    fn unknown_type_in_cast_reports_warning() {
        let (valid, _, _, _, _, warnings) =
            validate_sql_impl("SELECT CAST(x AS bignum) FROM t", &trino());
        assert!(valid);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "type");
        assert_eq!(warnings[0].1, "bignum");
    }

    #[test]
    fn function_and_type_warnings_coexist() {
        let (_, _, _, _, _, warnings) =
            validate_sql_impl("CREATE TABLE t (a marh, b bigint, c zort)", &trino());
        let kinds: Vec<_> = warnings.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(kinds, vec!["type", "type"]);
    }

    #[test]
    fn type_checking_only_applies_to_trino() {
        let (_, _, _, _, _, warnings) =
            validate_sql_impl("CREATE TABLE t (a bignum)", &SqlDialect::Generic);
        assert!(warnings.is_empty());
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
