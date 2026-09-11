use core::ops::ControlFlow;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use sqlparser::ast::{visit_expressions, FunctionArgumentClause, FunctionArguments};
use sqlparser::ast::{ArrayElemTypeDef, DataType, Expr, Ident, ObjectNamePart, Statement};
use sqlparser::ast::{JsonTableColumn, TableFactor, Visit, Visitor};
use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::str::FromStr;
use std::sync::LazyLock;

use crate::dialects::{parse_trino_sql, SqlDialect};
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer};

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

type StatementInfoTuple = (usize, usize, usize, usize, usize, String, Option<String>);

type StatementAnalysisTuple = (
    ValidationResultTuple,
    Vec<StatementInfoTuple>,
    Option<usize>,
);

static LOCATION_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"at Line: (\d+), Column: (\d+)").unwrap());

static LOCATION_SUFFIX_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s+at Line: \d+, Column: \d+$").unwrap());

const MAX_SQL_TOKENS: usize = 65_536;
const MAX_STATEMENT_TOKENS: usize = 4_096;
const MAX_NESTING_DEPTH: usize = 256;

fn complexity_error(message: &str, token: &TokenWithSpan) -> ValidationResultTuple {
    (
        false,
        0,
        Some(format!("sql parser error: {message}")),
        Some(token.span.start.line as usize),
        Some(token.span.start.column as usize),
        Vec::new(),
    )
}

fn validate_input_complexity(
    sql: &str,
    dialect: &dyn sqlparser::dialect::Dialect,
) -> Option<ValidationResultTuple> {
    let Ok(tokens) = Tokenizer::new(dialect, sql).tokenize_with_location() else {
        return None;
    };
    let mut total_tokens = 0;
    let mut statement_tokens = 0;
    let mut group_depth = 0;
    let mut compound_depth = 0;
    for token in &tokens {
        if matches!(token.token, Token::Whitespace(_)) {
            continue;
        }
        total_tokens += 1;
        statement_tokens += 1;
        if total_tokens > MAX_SQL_TOKENS {
            return Some(complexity_error(
                "maximum SQL input complexity exceeded",
                token,
            ));
        }
        if statement_tokens > MAX_STATEMENT_TOKENS {
            return Some(complexity_error(
                "maximum SQL statement complexity exceeded",
                token,
            ));
        }
        match &token.token {
            Token::LParen | Token::LBracket | Token::LBrace => {
                group_depth += 1;
                if group_depth > MAX_NESTING_DEPTH {
                    return Some(complexity_error("maximum nesting depth exceeded", token));
                }
            }
            Token::RParen | Token::RBracket | Token::RBrace => {
                group_depth = group_depth.saturating_sub(1);
            }
            Token::Word(word) if word.quote_style.is_none() => {
                if word.value.eq_ignore_ascii_case("BEGIN") {
                    compound_depth += 1;
                    if compound_depth > MAX_NESTING_DEPTH {
                        return Some(complexity_error("maximum nesting depth exceeded", token));
                    }
                } else if word.value.eq_ignore_ascii_case("END") {
                    compound_depth = compound_depth.saturating_sub(1);
                }
            }
            Token::SemiColon if compound_depth == 0 && group_depth == 0 => {
                statement_tokens = 0;
            }
            _ => {}
        }
    }
    None
}

fn empty_from_clause_location(
    sql: &str,
    dialect: &dyn sqlparser::dialect::Dialect,
) -> Option<(usize, usize)> {
    let Ok(tokens) = Tokenizer::new(dialect, sql).tokenize_with_location() else {
        return None;
    };
    let significant: Vec<&TokenWithSpan> = tokens
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .collect();
    significant.windows(2).find_map(|pair| {
        matches!(&pair[0].token, Token::Word(from)
            if from.keyword == sqlparser::keywords::Keyword::FROM
        )
        .then_some(())?;
        matches!(&pair[1].token, Token::Word(next)
                if next.quote_style.is_none()
                && matches!(next.value.to_ascii_uppercase().as_str(), "WHERE" | "GROUP" | "ORDER" | "HAVING" | "UNION" | "EXCEPT" | "INTERSECT")
        ).then_some((
            pair[1].span.start.line as usize,
            pair[1].span.start.column as usize,
        ))
    })
}

fn validate_sql_inner(sql: &str, dialect: &SqlDialect) -> ValidationResultTuple {
    let sql = sql.strip_prefix('\u{feff}').unwrap_or(sql);
    let parser = dialect.parser();
    if let Some(result) = validate_input_complexity(sql, parser.as_ref()) {
        return result;
    }
    if let Some((line, column)) = (*dialect == SqlDialect::Trino)
        .then(|| empty_from_clause_location(sql, parser.as_ref()))
        .flatten()
    {
        return (
            false,
            0,
            Some("sql parser error: FROM clause is missing a relation".to_string()),
            Some(line),
            Some(column),
            Vec::new(),
        );
    }
    let parsed = if *dialect == SqlDialect::Trino {
        parse_trino_sql(parser.as_ref(), sql).map(|parsed| {
            (
                parsed.statements,
                parsed.inline_functions,
                parsed.compatibility_metadata,
                parsed.custom_statement_kinds,
                parsed.source_type_names,
            )
        })
    } else {
        Parser::parse_sql(parser.as_ref(), sql)
            .map(|statements| (statements, Vec::new(), Vec::new(), Vec::new(), Vec::new()))
    };
    match parsed {
        Ok((
            statements,
            inline_functions,
            compatibility_metadata,
            custom_statement_kinds,
            source_type_names,
        )) => {
            debug_assert!(custom_statement_kinds.len() <= statements.len());
            let mut warnings = if *dialect == SqlDialect::Trino {
                let mut warning_statements = statements.clone();
                warning_statements.extend(
                    inline_functions
                        .iter()
                        .map(|(_, statement)| statement.clone()),
                );
                warning_statements.extend(compatibility_metadata.clone());
                let mut warnings = Vec::new();
                for (statement_index, statement) in statements.iter().enumerate() {
                    let declarations: Vec<Statement> = inline_functions
                        .iter()
                        .filter(|(scope, _)| *scope == statement_index)
                        .map(|(_, declaration)| declaration.clone())
                        .collect();
                    let local_function_names = inline_function_names(&declarations);
                    warnings.extend(find_unknown_functions(
                        std::slice::from_ref(statement),
                        &local_function_names,
                    ));
                    warnings.extend(find_unknown_functions(&declarations, &local_function_names));
                }
                warnings.extend(find_unknown_functions(
                    &compatibility_metadata,
                    &HashSet::new(),
                ));
                find_unknown_types(&warning_statements, &mut warnings);
                for ident in source_type_names {
                    let name = ident.value.to_ascii_lowercase();
                    let (line, column) = span_position(&ident);
                    warnings.push(("type".to_string(), name, line, column));
                }
                warnings
            } else {
                Vec::new()
            };
            warnings.sort_by_key(|w| (w.2.unwrap_or(usize::MAX), w.3.unwrap_or(usize::MAX)));
            warnings.dedup();
            (true, statements.len(), None, None, None, warnings)
        }
        Err(err) => {
            let (message, line, column) = error_details(err);
            (false, 0, Some(message), line, column, Vec::new())
        }
    }
}

pub fn validate_sql_impl(sql: &str, dialect: &SqlDialect) -> ValidationResultTuple {
    catch_unwind(AssertUnwindSafe(|| validate_sql_inner(sql, dialect))).unwrap_or_else(|_| {
        (
            false,
            0,
            Some("sql parser error: parser failed safely".to_string()),
            None,
            None,
            Vec::new(),
        )
    })
}

/// Walk every expression in the parsed statements and collect function calls
/// whose name is not in the Trino catalog, with the call site's line/column.
fn find_unknown_functions(
    statements: &[Statement],
    local_function_names: &HashSet<String>,
) -> Vec<(String, String, Option<usize>, Option<usize>)> {
    let mut unknown = Vec::new();
    let owned_statements = statements.to_vec();
    let _ = visit_expressions(&owned_statements, |expr| {
        if let Expr::Function(func) = expr {
            if let Some(ident) = func.name.0.last().and_then(|part| part.as_ident()) {
                let name = ident.value.to_ascii_lowercase();
                let is_unqualified_local =
                    func.name.0.len() == 1 && local_function_names.contains(&name);
                if !is_unqualified_local && !is_known_trino_function(&name) {
                    let (line, column) = span_position(ident);
                    unknown.push(("function".to_string(), name, line, column));
                }
            }
        }
        ControlFlow::<()>::Continue(())
    });
    unknown
}

fn inline_function_names(statements: &[Statement]) -> HashSet<String> {
    statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::CreateFunction(function) => function
                .name
                .0
                .last()
                .and_then(|part| part.as_ident())
                .map(|ident| ident.value.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
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
    statements: &[Statement],
    warnings: &mut Vec<(String, String, Option<usize>, Option<usize>)>,
) {
    struct TypeVisitor<'a> {
        warnings: &'a mut Vec<(String, String, Option<usize>, Option<usize>)>,
    }

    impl Visitor for TypeVisitor<'_> {
        type Break = ();

        fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
            match statement {
                Statement::CreateTable(create) => {
                    for column in &create.columns {
                        collect_type_entries(&column.data_type, self.warnings);
                    }
                }
                Statement::CreateView(create) => {
                    for column in &create.columns {
                        if let Some(data_type) = &column.data_type {
                            collect_type_entries(data_type, self.warnings);
                        }
                    }
                }
                Statement::AlterTable(alter) => {
                    for operation in &alter.operations {
                        match operation {
                            sqlparser::ast::AlterTableOperation::AddColumn {
                                column_def, ..
                            } => {
                                collect_type_entries(&column_def.data_type, self.warnings);
                            }
                            sqlparser::ast::AlterTableOperation::ChangeColumn {
                                data_type, ..
                            }
                            | sqlparser::ast::AlterTableOperation::ModifyColumn {
                                data_type, ..
                            } => {
                                collect_type_entries(data_type, self.warnings);
                            }
                            sqlparser::ast::AlterTableOperation::AlterColumn {
                                op:
                                    sqlparser::ast::AlterColumnOperation::SetDataType {
                                        data_type, ..
                                    },
                                ..
                            } => {
                                collect_type_entries(data_type, self.warnings);
                            }
                            sqlparser::ast::AlterTableOperation::AlterColumn { .. } => {}
                            _ => {}
                        }
                    }
                }
                Statement::CreateFunction(func) => {
                    if let Some(args) = &func.args {
                        for arg in args {
                            collect_type_entries(&arg.data_type, self.warnings);
                        }
                    }
                    if let Some(return_type) = &func.return_type {
                        match return_type {
                            sqlparser::ast::FunctionReturnType::DataType(data_type)
                            | sqlparser::ast::FunctionReturnType::SetOf(data_type) => {
                                collect_type_entries(data_type, self.warnings);
                            }
                        }
                    }
                }
                Statement::DropFunction(drop) => {
                    for function in &drop.func_desc {
                        for argument in function.args.iter().flatten() {
                            collect_type_entries(&argument.data_type, self.warnings);
                        }
                    }
                }
                Statement::Prepare { data_types, .. } => {
                    for data_type in data_types {
                        collect_type_entries(data_type, self.warnings);
                    }
                }
                Statement::Declare { stmts } => {
                    for declaration in stmts {
                        if let Some(data_type) = &declaration.data_type {
                            collect_type_entries(data_type, self.warnings);
                        }
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
            match expr {
                Expr::Cast { data_type, .. } => collect_type_entries(data_type, self.warnings),
                Expr::TypedString(typed) => {
                    collect_type_entries(&typed.data_type, self.warnings);
                }
                Expr::Function(function) => {
                    if let FunctionArguments::List(arguments) = &function.args {
                        for clause in &arguments.clauses {
                            if let FunctionArgumentClause::JsonReturningClause(returning) = clause {
                                collect_type_entries(&returning.data_type, self.warnings);
                            }
                        }
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_table_factor(
            &mut self,
            table_factor: &TableFactor,
        ) -> ControlFlow<Self::Break> {
            if let TableFactor::JsonTable { columns, .. } = table_factor {
                collect_json_table_types(columns, self.warnings);
            }
            ControlFlow::Continue(())
        }
    }

    let mut visitor = TypeVisitor { warnings };
    let _ = statements.to_vec().visit(&mut visitor);
}

fn collect_json_table_types(
    columns: &[JsonTableColumn],
    warnings: &mut Vec<(String, String, Option<usize>, Option<usize>)>,
) {
    for column in columns {
        match column {
            JsonTableColumn::Named(named) => collect_type_entries(&named.r#type, warnings),
            JsonTableColumn::Nested(nested) => collect_json_table_types(&nested.columns, warnings),
            JsonTableColumn::ForOrdinality(_) => {}
        }
    }
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
        Some(captures) => {
            let line = captures.get(1).and_then(|m| m.as_str().parse().ok());
            let column = captures.get(2).and_then(|m| m.as_str().parse().ok());
            if line == Some(0) || column == Some(0) {
                (None, None)
            } else {
                (line, column)
            }
        }
        None => (None, None),
    }
}

fn statement_kind(tokens: &[&TokenWithSpan]) -> (String, Option<String>) {
    let words: Vec<String> = tokens
        .iter()
        .filter_map(|token| match &token.token {
            Token::Word(word) if word.quote_style.is_none() => {
                Some(word.value.to_ascii_lowercase())
            }
            _ => None,
        })
        .collect();
    let classify = |words: &[String]| -> String {
        let Some(first) = words.first() else {
            return "unknown".to_string();
        };
        match first.as_str() {
            "select" | "table" | "values" | "with" => "query".to_string(),
            "create" => {
                let mut index = 1;
                if words.get(index).is_some_and(|word| word == "or") {
                    index += 2;
                }
                if words.get(index).is_some_and(|word| word == "materialized") {
                    "create_materialized_view".to_string()
                } else {
                    words
                        .get(index)
                        .map_or_else(|| "create".to_string(), |word| format!("create_{word}"))
                }
            }
            "alter" | "drop" => {
                if words.get(1).is_some_and(|word| word == "materialized") {
                    format!("{first}_materialized_view")
                } else {
                    words
                        .get(1)
                        .map_or_else(|| first.clone(), |word| format!("{first}_{word}"))
                }
            }
            "set" | "reset" => words
                .get(1)
                .map_or_else(|| first.clone(), |word| format!("{first}_{word}")),
            _ => first.clone(),
        }
    };
    let kind = classify(&words);
    let inner_start = match words.first().map(String::as_str) {
        Some("prepare") => words
            .iter()
            .position(|word| word == "from")
            .map(|index| index + 1),
        Some("explain") => words.iter().enumerate().skip(1).find_map(|(index, word)| {
            [
                "alter", "call", "create", "delete", "drop", "insert", "merge", "select", "set",
                "show", "table", "update", "values", "with",
            ]
            .contains(&word.as_str())
            .then_some(index)
        }),
        _ => None,
    };
    let inner = inner_start.map(|index| classify(&words[index..]));
    (kind, inner)
}

fn statement_info(sql: &str, dialect: &SqlDialect) -> Vec<StatementInfoTuple> {
    let parser = dialect.parser();
    let Ok(tokens) = Tokenizer::new(parser.as_ref(), sql).tokenize_with_location() else {
        return Vec::new();
    };
    let significant: Vec<&TokenWithSpan> = tokens
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_) | Token::EOF))
        .collect();
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut group_depth = 0usize;
    let mut routine = false;
    let mut routine_controls = Vec::new();
    let mut after_end = false;
    for (index, token) in significant.iter().enumerate() {
        match &token.token {
            Token::LParen | Token::LBracket | Token::LBrace => group_depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                group_depth = group_depth.saturating_sub(1);
            }
            Token::Word(word) if word.quote_style.is_none() => {
                let value = word.value.to_ascii_lowercase();
                if index.saturating_sub(start) < 5 && value == "function" {
                    routine = significant[start..index].iter().any(|candidate| {
                        matches!(&candidate.token, Token::Word(word) if word.value.eq_ignore_ascii_case("create"))
                    });
                }
                if routine {
                    if value == "end" {
                        routine_controls.pop();
                        after_end = true;
                    } else if ["begin", "case", "if", "loop", "repeat", "while"]
                        .contains(&value.as_str())
                    {
                        let next_is_group = significant
                            .get(index + 1)
                            .is_some_and(|next| next.token == Token::LParen);
                        if after_end {
                            after_end = false;
                        } else if value == "begin"
                            || (!next_is_group && !routine_controls.is_empty())
                        {
                            routine_controls.push(value);
                        }
                    } else if after_end {
                        after_end = false;
                    }
                }
            }
            Token::SemiColon if group_depth == 0 && (!routine || routine_controls.is_empty()) => {
                if start < index {
                    push_statement_info(&mut result, &significant[start..index]);
                }
                start = index + 1;
                routine = false;
                after_end = false;
            }
            _ => {}
        }
    }
    if start < significant.len() {
        push_statement_info(&mut result, &significant[start..]);
    }
    result
}

fn push_statement_info(result: &mut Vec<StatementInfoTuple>, tokens: &[&TokenWithSpan]) {
    let Some(first) = tokens.first() else {
        return;
    };
    let Some(last) = tokens.last() else {
        return;
    };
    let (kind, inner_kind) = statement_kind(tokens);
    result.push((
        result.len(),
        first.span.start.line as usize,
        first.span.start.column as usize,
        last.span.end.line as usize,
        last.span.end.column as usize,
        kind,
        inner_kind,
    ));
}

fn error_statement_index(
    validation: &ValidationResultTuple,
    statements: &[StatementInfoTuple],
) -> Option<usize> {
    let (Some(line), Some(column)) = (validation.3, validation.4) else {
        return None;
    };
    statements
        .iter()
        .rev()
        .find(|statement| (statement.1, statement.2) <= (line, column))
        .map(|statement| statement.0)
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

/// Validate SQL and return opt-in source metadata for each statement.
#[pyfunction]
#[pyo3(signature = (sql, dialect = "trino"))]
fn analyze_statements(sql: &str, dialect: &str) -> PyResult<StatementAnalysisTuple> {
    let parsed_dialect = SqlDialect::from_str(dialect)?;
    let validation = validate_sql_impl(sql, &parsed_dialect);
    let statements = statement_info(sql, &parsed_dialect);
    let error_index = error_statement_index(&validation, &statements);
    Ok((validation, statements, error_index))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", PACKAGE_VERSION)?;
    m.add("__doc__", "Rust-native core for trino_sql_validator.")?;
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(validate_file, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_statements, m)?)?;
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
    fn statement_metadata_uses_source_kinds_and_routine_boundaries() {
        let sql = "CREATE FUNCTION f(x BIGINT) RETURNS BIGINT BEGIN DECLARE y BIGINT; IF x > 0 THEN RETURN x; END IF; RETURN y; END; EXPLAIN ALTER TABLE t SET PROPERTIES x = 1";
        let statements = statement_info(sql, &trino());
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].0, 0);
        assert_eq!(statements[0].5, "create_function");
        assert_eq!(statements[0].6, None);
        assert_eq!(statements[1].0, 1);
        assert_eq!(statements[1].5, "explain");
        assert_eq!(statements[1].6.as_deref(), Some("alter_table"));
    }

    #[test]
    fn zero_parser_locations_are_not_exposed() {
        assert_eq!(
            extract_location("error at Line: 0, Column: 0"),
            (None, None)
        );
    }

    #[test]
    fn excessive_expression_complexity_fails_safely_for_all_dialects() {
        let sql = format!("SELECT {}", vec!["1"; 2_049].join(" + "));
        for dialect in [SqlDialect::Trino, SqlDialect::Hive, SqlDialect::Generic] {
            let (valid, count, message, line, column, warnings) = validate_sql_impl(&sql, &dialect);
            assert!(!valid);
            assert_eq!(count, 0);
            assert_eq!(
                message.as_deref(),
                Some("sql parser error: maximum SQL statement complexity exceeded")
            );
            assert_eq!(line, Some(1));
            assert!(column.is_some());
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn excessive_routine_nesting_fails_safely() {
        let sql = format!(
            "CREATE FUNCTION f() RETURNS BIGINT {}RETURN 1;{}",
            "BEGIN ".repeat(MAX_NESTING_DEPTH + 1),
            " END;".repeat(MAX_NESTING_DEPTH + 1)
        );
        let (valid, count, message, line, column, warnings) = validate_sql_impl(&sql, &trino());
        assert!(!valid);
        assert_eq!(count, 0);
        assert_eq!(
            message.as_deref(),
            Some("sql parser error: maximum nesting depth exceeded")
        );
        assert_eq!(line, Some(1));
        assert!(column.is_some());
        assert!(warnings.is_empty());
    }

    #[test]
    fn many_independent_statements_do_not_share_the_statement_budget() {
        let sql = "SELECT 1;".repeat(1_000);
        let (valid, count, message, _, _, warnings) = validate_sql_impl(&sql, &trino());
        assert!(valid, "unexpected error: {message:?}");
        assert_eq!(count, 1_000);
        assert!(warnings.is_empty());
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
            assert_eq!(warnings.len(), 1, "{warnings:?}");
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
            assert!(warnings.is_empty(), "{warnings:?}");
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
    fn trino_accepts_legacy_angle_map_type_syntax() {
        let (valid, _, _, _, _, _) =
            validate_sql_impl("CREATE TABLE t (value map<varchar, bigint>)", &trino());
        assert!(valid);
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
