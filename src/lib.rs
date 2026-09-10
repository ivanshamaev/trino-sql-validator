use core::ops::ControlFlow;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use sqlparser::ast::visit_expressions;
use sqlparser::ast::{ArrayElemTypeDef, DataType, Expr, Ident, ObjectNamePart, Statement};
use std::str::FromStr;
use std::sync::LazyLock;

use crate::dialects::{parse_trino_sql, SqlDialect};
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Token, Tokenizer};

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

static ARRAY_TYPE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(\bAS\s+ARRAY)\s*\(\s*[a-zA-Z_][a-zA-Z0-9_]*(?:\s+WITH\s+TIME\s+ZONE)?\s*\)",
    )
    .unwrap()
});

static TOP_QUALIFIED_IDENTIFIER_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\btop\s*(\.)").unwrap());

static TOP_ALIAS_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)(\bAS\s+)top\b|(\))\s+top\b").unwrap());

static IPADDRESS_LITERAL_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\bIPADDRESS\s*('[^']*')").unwrap());

static ICEBERG_VERSION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\bFOR\s+VERSION\s+AS\s+OF\b").unwrap());

static ICEBERG_TIMESTAMP_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\bFOR(\s+TIMESTAMP\s+AS\s+OF\b)").unwrap());

static ICEBERG_NAMED_VERSION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)(\bVERSION\s+AS\s+OF)\s+'[^']*'").unwrap());

static SCALAR_VALUES_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?is)\(\s*VALUES\s+((?:'[^']*'\s*,\s*)+'[^']*')\s*\)").unwrap()
});

static TYPED_VALUES_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)\bVALUES\s+VARCHAR\s+('(?:''|[^'])*')").unwrap());

static ARRAY_VALUES_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)\bVALUES\s+(ARRAY\s*\[[^\]]*\])").unwrap());

static FUNCTION_VALUES_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?is)\bVALUES\s+(map_from_entries\s*\(\s*ARRAY\s*\[[^\]]*\]\s*\))").unwrap()
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

fn normalize_array_types(sql: &str) -> String {
    ARRAY_TYPE_PATTERN
        .replace_all(sql, |captures: &regex::Captures<'_>| {
            let mut value = captures.get(0).unwrap().as_str().to_string();
            if let Some(open) = value.rfind('(') {
                value.replace_range(open..=open, "<");
            }
            if let Some(close) = value.rfind(')') {
                value.replace_range(close..=close, ">");
            }
            value
        })
        .into()
}

fn normalize_top_identifiers(sql: &str) -> String {
    let sql = TOP_QUALIFIED_IDENTIFIER_PATTERN.replace_all(sql, "\"top\"$1");
    TOP_ALIAS_PATTERN
        .replace_all(&sql, |captures: &regex::Captures<'_>| {
            if let Some(prefix) = captures.get(1) {
                format!("{}\"top\"", prefix.as_str())
            } else {
                ") \"top\"".to_string()
            }
        })
        .into()
}

fn normalize_ipaddress_literals(sql: &str) -> String {
    IPADDRESS_LITERAL_PATTERN
        .replace_all(sql, "CAST($1 AS IPADDRESS)")
        .into()
}

fn normalize_iceberg_version_as_of(sql: &str) -> String {
    ICEBERG_VERSION_PATTERN
        .replace_all(sql, "VERSION AS OF")
        .into()
}

fn normalize_iceberg_timestamp_as_of(sql: &str) -> String {
    ICEBERG_TIMESTAMP_PATTERN.replace_all(sql, "   $1").into()
}

fn normalize_iceberg_named_versions(sql: &str) -> String {
    ICEBERG_NAMED_VERSION_PATTERN
        .replace_all(sql, "$1 0")
        .into()
}

fn normalize_scalar_values(sql: &str) -> String {
    SCALAR_VALUES_PATTERN
        .replace_all(sql, |captures: &regex::Captures<'_>| {
            let values = captures.get(1).unwrap().as_str();
            let rows = values
                .split(',')
                .map(|value| format!("({})", value.trim()))
                .collect::<Vec<_>>()
                .join(", ");
            format!("(VALUES {rows})")
        })
        .into()
}

fn normalize_typed_values(sql: &str) -> String {
    TYPED_VALUES_PATTERN
        .replace_all(sql, "VALUES (CAST($1 AS VARCHAR))")
        .into()
}

fn normalize_array_values(sql: &str) -> String {
    ARRAY_VALUES_PATTERN.replace_all(sql, "VALUES ($1)").into()
}

fn normalize_function_values(sql: &str) -> String {
    FUNCTION_VALUES_PATTERN
        .replace_all(sql, "VALUES ($1)")
        .into()
}

fn has_empty_from_clause(sql: &str, dialect: &dyn sqlparser::dialect::Dialect) -> bool {
    let mut tokenizer = Tokenizer::new(dialect, sql);
    let Ok(tokens) = tokenizer.tokenize() else {
        return false;
    };
    let significant: Vec<&Token> = tokens
        .iter()
        .filter(|token| !matches!(token, Token::Whitespace(_)))
        .collect();
    significant.windows(2).any(|pair| {
        matches!(pair, [Token::Word(from), Token::Word(next)]
            if from.keyword == sqlparser::keywords::Keyword::FROM
                && next.quote_style.is_none()
                && matches!(next.value.to_ascii_uppercase().as_str(), "WHERE" | "GROUP" | "ORDER" | "HAVING" | "LIMIT" | "OFFSET" | "UNION" | "EXCEPT" | "INTERSECT"))
    })
}

pub fn validate_sql_impl(sql: &str, dialect: &SqlDialect) -> ValidationResultTuple {
    let parser = dialect.parser();
    if *dialect == SqlDialect::Trino && has_empty_from_clause(sql, parser.as_ref()) {
        return (
            false,
            0,
            Some("sql parser error: FROM clause is missing a relation".to_string()),
            None,
            None,
            Vec::new(),
        );
    }
    let sql = if *dialect == SqlDialect::Trino {
        normalize_top_identifiers(&normalize_ipaddress_literals(
            &normalize_iceberg_named_versions(&normalize_iceberg_timestamp_as_of(
                &normalize_iceberg_version_as_of(&normalize_function_values(
                    &normalize_array_values(&normalize_typed_values(&normalize_scalar_values(
                        &normalize_array_types(&normalize_prepare_from(sql)),
                    ))),
                )),
            )),
        ))
    } else {
        sql.to_string()
    };
    let parsed = if *dialect == SqlDialect::Trino {
        parse_trino_sql(parser.as_ref(), &sql)
    } else {
        Parser::parse_sql(parser.as_ref(), &sql)
    };
    match parsed {
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
                if !is_known_trino_function(&name) {
                    let (line, column) = span_position(ident);
                    unknown.push(("function".to_string(), name, line, column));
                }
            }
        }
        ControlFlow::<()>::Continue(())
    });
    unknown
}

fn is_known_trino_function(name: &str) -> bool {
    functions::is_known_function(name)
        || matches!(
            name,
            "classifier"
                | "descriptor"
                | "first"
                | "last"
                | "match_number"
                | "next"
                | "prev"
                | "table"
                | "table_changes"
        )
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
    fn trino_accepts_array_parenthesis_type_syntax() {
        for sql in [
            "SELECT CAST(SPLIT(x, ',') AS ARRAY (BIGINT)) FROM t",
            "WITH RECURSIVE h(id, path) AS (SELECT 1, CAST(ARRAY[] AS ARRAY(VARCHAR)) UNION ALL SELECT id, CAST(ARRAY[id] AS ARRAY(VARCHAR)) FROM h) SELECT * FROM h",
        ] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(valid, "expected to parse: {sql}");
        }
    }

    #[test]
    fn trino_accepts_ipaddress_literals() {
        let (valid, _, _, _, _, warnings) = validate_sql_impl(
            "SELECT contains('10.0.0.0/8', IPADDRESS '11.255.255.255')",
            &trino(),
        );
        assert!(valid);
        assert!(warnings.is_empty());
    }

    #[test]
    fn trino_accepts_iceberg_version_time_travel() {
        let (valid, count, _, _, _, _) =
            validate_sql_impl("SELECT * FROM customer FOR VERSION AS OF 1", &trino());
        assert!(valid);
        assert_eq!(count, 1);
    }

    #[test]
    fn trino_accepts_with_session() {
        for sql in [
            "WITH SESSION query_max_execution_time = '2h' SELECT * FROM orders",
            "WITH SESSION example.query_partition_filter_required = true, query_max_memory = 1 + 2 SELECT marh(id) FROM orders",
            "WITH SESSION x = ARRAY[1, 2] WITH t AS (SELECT 1 AS id) SELECT * FROM t",
        ] {
            let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(count, 1);
            if sql.contains("marh") {
                assert_eq!(warnings.len(), 1);
                assert_eq!(warnings[0].1, "marh");
                assert_eq!(warnings[0].3, sql.find("marh").map(|index| index + 1));
            } else {
                assert!(warnings.is_empty());
            }
        }
    }

    #[test]
    fn trino_rejects_malformed_with_session() {
        for sql in [
            "WITH SESSION SELECT 1",
            "WITH SESSION query_max_memory SELECT 1",
            "WITH SESSION query_max_memory = SELECT 1",
            "WITH SESSION query_max_memory = 1, SELECT 1",
        ] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
    }

    #[test]
    fn trino_accepts_iceberg_dml_branch_references() {
        for sql in [
            "INSERT INTO customer @ dev (id) VALUES (1)",
            "DELETE FROM customer @ dev WHERE id = 1",
            "UPDATE catalog.schema.customer @ dev SET id = 1",
            "MERGE INTO target @ dev USING source ON target.id = source.id WHEN MATCHED THEN DELETE",
        ] {
            let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(count, 1);
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn trino_does_not_treat_arbitrary_at_words_as_branch_references() {
        for sql in ["@select", "SELECT @branch", "DELETE @ branch"] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
    }

    #[test]
    fn trino_rejects_invalid_identifier_shapes() {
        for sql in ["SELECT 1x FROM dual", "SELECT \"\"", "SELECT * FROM \"\""] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
        for sql in ["SELECT 1 x FROM dual", "SELECT \"1x\"", "SELECT ''"] {
            let (valid, _, message, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
        }
    }

    #[test]
    fn trino_accepts_non_decimal_integer_literals() {
        for sql in [
            "SELECT 0X123_ABC_DEF",
            "SELECT -0x123_abc_def",
            "SELECT 0O012_345",
            "SELECT -0o012_345",
            "SELECT 0B110_010",
            "SELECT -0b110_010",
        ] {
            let (valid, _, message, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
        }
        for sql in ["SELECT 0X123_G", "SELECT 0O018", "SELECT 0B102"] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
    }

    #[test]
    fn trino_branch_normalization_preserves_source_positions() {
        for sql in [
            "UPDATE customer @ dev SET score = marh(1)",
            "SELECT '@branch', marh(1)",
            "SELECT 1 -- @branch\n, marh(1)",
        ] {
            let (valid, _, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(warnings.len(), 1);
            assert_eq!(warnings[0].1, "marh");
            let prefix = &sql[..sql.find("marh").unwrap()];
            let expected_line = prefix.matches('\n').count() + 1;
            let expected_column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
            assert_eq!(warnings[0].2, Some(expected_line));
            assert_eq!(warnings[0].3, Some(expected_column));
        }
    }

    #[test]
    fn trino_accepts_iceberg_timestamp_time_travel() {
        for sql in [
            "SELECT * FROM customer FOR TIMESTAMP AS OF TIMESTAMP '2022-03-23 09:59:29.803 Europe/Vienna'",
            "SELECT * FROM customer FOR TIMESTAMP AS OF DATE '2022-03-23'",
            "SELECT * FROM customer FOR TIMESTAMP AS OF current_timestamp - INTERVAL '1' DAY",
        ] {
            let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(count, 1);
            assert!(warnings.is_empty());
        }

        let sql =
            "SELECT * FROM customer FOR TIMESTAMP AS OF DATE '2022-03-23' WHERE marh(custkey)";
        let (valid, _, message, _, _, warnings) = validate_sql_impl(sql, &trino());
        assert!(valid, "unexpected error: {message:?}");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].1, "marh");
        assert_eq!(warnings[0].3, sql.find("marh").map(|offset| offset + 1));
    }

    #[test]
    fn trino_rejects_incomplete_iceberg_timestamp_time_travel() {
        for sql in [
            "SELECT * FROM customer FOR TIMESTAMP AS OF",
            "SELECT * FROM customer FOR TIMESTAMP AS OF +",
            "SELECT * FROM customer FOR TIMESTAMP AS OF TIMESTAMP 'unterminated",
        ] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
    }

    #[test]
    fn trino_accepts_materialized_view_staleness_options() {
        for sql in [
            "CREATE MATERIALIZED VIEW orders_summary GRACE PERIOD INTERVAL '1' HOUR WHEN STALE FAIL AS SELECT orderdate, sum(totalprice) AS price FROM orders GROUP BY orderdate",
            "CREATE MATERIALIZED VIEW orders_summary WHEN STALE INLINE AS SELECT * FROM orders",
            "CREATE OR REPLACE MATERIALIZED VIEW orders_summary GRACE PERIOD INTERVAL '1' DAY WHEN STALE FAIL COMMENT 'daily summary' WITH (format = 'ORC') AS SELECT * FROM orders",
        ] {
            let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(count, 1);
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn trino_rejects_malformed_materialized_view_staleness_options() {
        for sql in [
            "CREATE MATERIALIZED VIEW v GRACE PERIOD WHEN STALE FAIL AS SELECT 1",
            "CREATE MATERIALIZED VIEW v GRACE PERIOD nonsense WHEN STALE FAIL AS SELECT 1",
            "CREATE MATERIALIZED VIEW v WHEN STALE UNKNOWN AS SELECT 1",
            "CREATE MATERIALIZED VIEW v COMMENT identifier AS SELECT 1",
            "CREATE MATERIALIZED VIEW v WHEN STALE FAIL GRACE PERIOD INTERVAL '1' HOUR AS SELECT 1",
        ] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
    }

    #[test]
    fn documented_special_expressions_are_known() {
        let (_, _, _, _, _, warnings) = validate_sql_impl(
            "SELECT current_date, current_timestamp, localtime, localtimestamp, grouping(a), histogram(x) FROM t GROUP BY GROUPING SETS ((a), ())",
            &trino(),
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn documented_json_functions_are_known() {
        let (_, _, _, _, _, warnings) = validate_sql_impl(
            "SELECT json_exists(x, 'lax $.value'), json_query(x, 'lax $'), json_value(x, 'lax $.value'), json_array(x), json_object('x' : x) FROM t",
            &trino(),
        );
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
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
    fn documented_table_and_pattern_functions_produce_no_warnings() {
        let sql = "
            SELECT * FROM TABLE(exclude_columns(
                input => TABLE(orders),
                columns => DESCRIPTOR(clerk, comment)
            ));
            SELECT * FROM TABLE(system.table_changes(
                schema_name => 'default',
                table_name => 't1',
                start_snapshot_id => 1,
                end_snapshot_id => 2
            ));
            SELECT * FROM orders MATCH_RECOGNIZE (
                PATTERN (A B+)
                DEFINE B AS totalprice < PREV(totalprice)
            )
        ";
        let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
        assert!(valid, "unexpected error: {message:?}");
        assert_eq!(count, 3);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn trino_accepts_match_recognize_subsets() {
        for sql in [
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (A B+ C+ D+) SUBSET U = (C, D) DEFINE B AS totalprice < PREV(totalprice), C AS totalprice > PREV(totalprice), D AS totalprice > PREV(totalprice))",
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (A B C) SUBSET U = (A, B), V = (B, C) DEFINE B AS totalprice > A.totalprice)",
        ] {
            let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(count, 1);
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn trino_rejects_malformed_match_recognize_subsets() {
        for sql in [
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (A B) SUBSET U = () DEFINE B AS true)",
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (A B) SUBSET U (A, B) DEFINE B AS true)",
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (A B) SUBSET U = (A, B) B AS true)",
        ] {
            let (valid, _, _, _, _, _) = validate_sql_impl(sql, &trino());
            assert!(!valid, "expected invalid SQL: {sql}");
        }
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
    fn nested_row_array_and_map_types_parse() {
        for sql in [
            "CREATE TABLE t (
                nested row(a row(b bigint)),
                rows array(row(c varchar)),
                keyed map(varchar, row(d array(row(e integer))))
            )",
            "SELECT CAST(x AS row(a row(b bigint))) FROM t",
            "CREATE FUNCTION f() RETURNS row(a row(b bigint)) RETURN ROW(ROW(1))",
            "ALTER TABLE t ADD COLUMN nested row(a row(b bigint))",
        ] {
            let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
            assert!(valid, "unexpected error for {sql}: {message:?}");
            assert_eq!(count, 1);
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn unknown_type_in_nested_row_preserves_position() {
        let sql = "CREATE TABLE t (a row(x array(row(y bignum))))";
        let (valid, _, message, _, _, warnings) = validate_sql_impl(sql, &trino());
        assert!(valid, "unexpected error: {message:?}");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "type");
        assert_eq!(warnings[0].1, "bignum");
        assert_eq!(warnings[0].2, Some(1));
        assert_eq!(warnings[0].3, Some(37));
    }

    #[test]
    fn row_value_constructors_are_not_rewritten_as_types() {
        let sql =
            "SELECT ROW(1, 2.0), ROW(current_date, current_timestamp), ROW(varchar, bigint) FROM t;
            CREATE TABLE nested_rows (value row(child row(id bigint)))";
        let (valid, count, message, _, _, warnings) = validate_sql_impl(sql, &trino());
        assert!(valid, "unexpected error: {message:?}");
        assert_eq!(count, 2);
        assert!(warnings.is_empty());
    }

    #[test]
    fn trino_rejects_generic_angle_map_type_syntax() {
        let (valid, _, _, _, _, _) =
            validate_sql_impl("CREATE TABLE t (value map<varchar, bigint>)", &trino());
        assert!(!valid);
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
