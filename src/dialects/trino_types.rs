use core::ops::ControlFlow;
use std::cmp::Reverse;

use sqlparser::ast::{
    ArrayElemTypeDef, BinaryOperator, ColumnOption, DataType, DateTimeField, FromTable, Interval,
};
use sqlparser::ast::{Expr, FunctionArgExpr, FunctionArgOperator, FunctionArguments, Ident};
use sqlparser::ast::{FunctionArg, FunctionReturnType, ObjectName, SchemaName, Spanned};
use sqlparser::ast::{JoinConstraint, JoinOperator, LimitClause, Query, Select, SelectItem};
use sqlparser::ast::{Statement, TableAlias, TableFactor};
use sqlparser::ast::{TableSampleKind, TableSampleMethod};
use sqlparser::ast::{UnaryOperator, Value, Visit, Visitor};
use sqlparser::dialect::{Dialect, GenericDialect};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer, Whitespace, Word};

use crate::dialects::trino_keywords;
use crate::types;

pub(crate) struct ParsedSql {
    pub(crate) statements: Vec<Statement>,
    pub(crate) inline_functions: Vec<(usize, Statement)>,
    pub(crate) compatibility_metadata: Vec<Statement>,
    pub(crate) custom_statement_kinds: Vec<TrinoStatementKind>,
    pub(crate) source_type_names: Vec<Ident>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrinoStatementKind {
    Alter,
    Catalog,
    Branch,
    Role,
    Privilege,
    Show,
    Describe,
    RefreshMaterializedView,
    Session,
    Path,
}

fn next_significant(tokens: &[TokenWithSpan], start: usize, end: usize) -> Option<usize> {
    (start..end).find(|index| !matches!(tokens[*index].token, Token::Whitespace(_)))
}

fn previous_significant(tokens: &[TokenWithSpan], end: usize) -> Option<usize> {
    (0..end)
        .rev()
        .find(|index| !matches!(tokens[*index].token, Token::Whitespace(_)))
}

fn matching_rparen(tokens: &[TokenWithSpan], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.token {
            Token::LParen => depth += 1,
            Token::RParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn matching_lparen(tokens: &[TokenWithSpan], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in (0..=close).rev() {
        match tokens[index].token {
            Token::RParen => depth += 1,
            Token::LParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn matching_gt(tokens: &[TokenWithSpan], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.token {
            Token::Lt => depth += 1,
            Token::Gt => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn unquoted_keyword(token: &TokenWithSpan) -> Option<Keyword> {
    match &token.token {
        Token::Word(word) if word.quote_style.is_none() => Some(word.keyword),
        _ => None,
    }
}

fn is_type_container(token: &TokenWithSpan) -> bool {
    matches!(
        unquoted_keyword(token),
        Some(Keyword::ROW | Keyword::ARRAY | Keyword::MAP)
    )
}

fn is_probable_type_name(token: &TokenWithSpan) -> bool {
    let Token::Word(word) = &token.token else {
        return false;
    };
    word.quote_style.is_some()
        || word.keyword == Keyword::NoKeyword
        || types::is_known_type(&word.value.to_ascii_lowercase())
}

fn is_known_type_name(token: &TokenWithSpan) -> bool {
    let Token::Word(word) = &token.token else {
        return false;
    };
    word.quote_style.is_none() && types::is_known_type(&word.value.to_ascii_lowercase())
}

fn row_group_looks_like_type(tokens: &[TokenWithSpan], open: usize, close: usize) -> bool {
    let Some(first) = next_significant(tokens, open + 1, close) else {
        return false;
    };
    let Some(second) = next_significant(tokens, first + 1, close) else {
        return is_known_type_name(&tokens[first]);
    };
    if matches!(tokens[first].token, Token::Word(_)) && is_probable_type_name(&tokens[second]) {
        return true;
    }
    is_known_type_name(&tokens[first])
        && matches!(
            tokens[second].token,
            Token::Comma | Token::LParen | Token::Lt
        )
}

fn enclosing_parentheses(tokens: &[TokenWithSpan], before: usize) -> Vec<usize> {
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate().take(before) {
        match token.token {
            Token::LParen => stack.push(index),
            Token::RParen => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack
}

fn statement_start(tokens: &[TokenWithSpan], before: usize) -> usize {
    tokens[..before]
        .iter()
        .rposition(|token| token.token == Token::SemiColon)
        .map_or(0, |index| index + 1)
}

fn statement_starts_with(tokens: &[TokenWithSpan], before: usize, keyword: Keyword) -> bool {
    let start = statement_start(tokens, before);
    next_significant(tokens, start, before)
        .is_some_and(|index| unquoted_keyword(&tokens[index]) == Some(keyword))
}

fn is_unquoted_word(token: &TokenWithSpan, expected: &str) -> bool {
    matches!(
        &token.token,
        Token::Word(word)
            if word.quote_style.is_none() && word.value.eq_ignore_ascii_case(expected)
    )
}

fn custom_statement_kind(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
) -> Option<TrinoStatementKind> {
    let first = next_significant(tokens, start, end)?;
    let second = next_significant(tokens, first + 1, end);
    if ["grant", "revoke", "deny"]
        .iter()
        .any(|word| is_unquoted_word(&tokens[first], word))
    {
        return Some(TrinoStatementKind::Privilege);
    }
    if is_unquoted_word(&tokens[first], "alter") {
        return Some(TrinoStatementKind::Alter);
    }
    if is_unquoted_word(&tokens[first], "describe")
        && second.is_some_and(|index| {
            is_unquoted_word(&tokens[index], "input") || is_unquoted_word(&tokens[index], "output")
        })
    {
        return Some(TrinoStatementKind::Describe);
    }
    if is_unquoted_word(&tokens[first], "refresh")
        && second.is_some_and(|index| is_unquoted_word(&tokens[index], "materialized"))
    {
        return Some(TrinoStatementKind::RefreshMaterializedView);
    }
    if is_unquoted_word(&tokens[first], "reset") {
        return second
            .is_some_and(|index| is_unquoted_word(&tokens[index], "session"))
            .then_some(TrinoStatementKind::Session);
    }
    if is_unquoted_word(&tokens[first], "set") {
        return second.and_then(|index| {
            if is_unquoted_word(&tokens[index], "path") {
                Some(TrinoStatementKind::Path)
            } else if is_unquoted_word(&tokens[index], "role") {
                Some(TrinoStatementKind::Role)
            } else if is_unquoted_word(&tokens[index], "session") {
                Some(TrinoStatementKind::Session)
            } else {
                None
            }
        });
    }
    if is_unquoted_word(&tokens[first], "show") {
        return Some(TrinoStatementKind::Show);
    }
    if !(is_unquoted_word(&tokens[first], "create") || is_unquoted_word(&tokens[first], "drop")) {
        return None;
    }
    let mut cursor = second?;
    if is_unquoted_word(&tokens[cursor], "or") {
        let replace = next_significant(tokens, cursor + 1, end)?;
        cursor = next_significant(tokens, replace + 1, end)?;
    }
    if is_unquoted_word(&tokens[cursor], "catalog") {
        Some(TrinoStatementKind::Catalog)
    } else if is_unquoted_word(&tokens[cursor], "branch") {
        Some(TrinoStatementKind::Branch)
    } else if is_unquoted_word(&tokens[cursor], "role") {
        Some(TrinoStatementKind::Role)
    } else {
        None
    }
}

fn collect_custom_statement_kinds(tokens: &[TokenWithSpan]) -> Vec<TrinoStatementKind> {
    let mut kinds = Vec::new();
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        if let Some(kind) = custom_statement_kind(tokens, start, end) {
            kinds.push(kind);
        }
        start = end.saturating_add(1);
    }
    kinds
}

fn syntax_error(token: &TokenWithSpan, message: &str) -> ParserError {
    ParserError::ParserError(format!(
        "{message} at Line: {}, Column: {}",
        token.span.start.line, token.span.start.column
    ))
}

fn validate_balanced_groups(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    const MAX_NESTING_DEPTH: usize = 256;
    let mut groups = Vec::new();
    for token in tokens {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => {
                let expected = match token.token {
                    Token::LParen => Token::RParen,
                    Token::LBracket => Token::RBracket,
                    Token::LBrace => Token::RBrace,
                    _ => unreachable!(),
                };
                groups.push((token, expected));
                if groups.len() > MAX_NESTING_DEPTH {
                    return Err(syntax_error(token, "maximum nesting depth exceeded"));
                }
            }
            Token::RParen | Token::RBracket | Token::RBrace => {
                let Some((_, expected)) = groups.pop() else {
                    return Err(syntax_error(token, "unbalanced closing group"));
                };
                if token.token != expected {
                    return Err(syntax_error(token, "mismatched closing group"));
                }
            }
            _ => {}
        }
    }
    if let Some((token, _)) = groups.pop() {
        return Err(syntax_error(token, "unterminated group"));
    }
    Ok(())
}

fn validate_trino_lexical_tokens(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for (index, token) in tokens.iter().enumerate() {
        match &token.token {
            Token::DoubleEq => {
                return Err(syntax_error(
                    token,
                    "Trino does not support the == operator",
                ));
            }
            Token::Placeholder(value) if value.starts_with('$') => {
                return Err(syntax_error(token, "Trino does not support $ placeholders"));
            }
            Token::HexStringLiteral(value) => {
                let compact_span = token.span.start.line == token.span.end.line
                    && token
                        .span
                        .end
                        .column
                        .saturating_sub(token.span.start.column)
                        == value.chars().count() as u64 + 2;
                let non_decimal_integer = compact_span
                    || previous_significant(tokens, index).is_some_and(|previous| {
                        matches!(&tokens[previous].token, Token::Number(number, false) if number == "0")
                            && tokens[previous].span.end == token.span.start
                    });
                if non_decimal_integer {
                    continue;
                }
                let digits = value.chars().filter(|character| !character.is_whitespace());
                if digits
                    .clone()
                    .any(|character| !character.is_ascii_hexdigit())
                    || digits.count() % 2 != 0
                {
                    return Err(syntax_error(
                        token,
                        "Trino binary literals require an even number of hexadecimal digits",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_empty_statement_segments(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    let significant: Vec<&TokenWithSpan> = tokens
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_) | Token::EOF))
        .collect();
    let mut segment_has_content = false;
    let mut group_depth = 0usize;
    let mut routine = false;
    let mut routine_controls: Vec<String> = Vec::new();
    let mut after_end = false;
    let mut segment_start = 0usize;

    for (index, token) in significant.iter().enumerate() {
        match &token.token {
            Token::LParen | Token::LBracket | Token::LBrace => group_depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                group_depth = group_depth.saturating_sub(1);
            }
            Token::Word(word) if word.quote_style.is_none() => {
                let value = word.value.to_ascii_lowercase();
                if index.saturating_sub(segment_start) < 5 && value == "function" {
                    routine = significant[segment_start..index].iter().any(|candidate| {
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
                segment_has_content = true;
            }
            Token::SemiColon if group_depth == 0 && (!routine || routine_controls.is_empty()) => {
                if !segment_has_content {
                    return Err(syntax_error(token, "empty SQL statement is not allowed"));
                }
                segment_has_content = false;
                segment_start = index + 1;
                routine = false;
                after_end = false;
            }
            _ => segment_has_content = true,
        }
    }
    Ok(())
}

fn validate_cte_as(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for with in 0..tokens.len() {
        if !is_unquoted_word(&tokens[with], "with") {
            continue;
        }
        let Some(mut name) = next_significant(tokens, with + 1, tokens.len()) else {
            continue;
        };
        if is_unquoted_word(&tokens[name], "session") || is_unquoted_word(&tokens[name], "function")
        {
            continue;
        }
        if is_unquoted_word(&tokens[name], "recursive") {
            let Some(next) = next_significant(tokens, name + 1, tokens.len()) else {
                continue;
            };
            name = next;
        }
        if !is_identifier(&tokens[name]) {
            continue;
        }
        while let Some(mut cursor) = next_significant(tokens, name + 1, tokens.len()) {
            if tokens[cursor].token == Token::LParen {
                let Some(close) = matching_rparen(tokens, cursor) else {
                    break;
                };
                let body_without_as =
                    next_significant(tokens, cursor + 1, close).is_some_and(|first| {
                        ["select", "table", "values", "with"]
                            .iter()
                            .any(|word| is_unquoted_word(&tokens[first], word))
                    });
                if body_without_as {
                    return Err(syntax_error(&tokens[cursor], "Trino CTE query requires AS"));
                }
                let Some(after_columns) = next_significant(tokens, close + 1, tokens.len()) else {
                    break;
                };
                cursor = after_columns;
            }
            if !is_unquoted_word(&tokens[cursor], "as") {
                if tokens[cursor].token == Token::LParen
                    && matching_rparen(tokens, cursor).is_some_and(|close| {
                        next_significant(tokens, cursor + 1, close).is_some_and(|first| {
                            ["select", "table", "values", "with"]
                                .iter()
                                .any(|word| is_unquoted_word(&tokens[first], word))
                        })
                    })
                {
                    return Err(syntax_error(&tokens[cursor], "Trino CTE query requires AS"));
                }
                break;
            }
            let Some(open) = next_significant(tokens, cursor + 1, tokens.len()) else {
                break;
            };
            if tokens[open].token != Token::LParen {
                break;
            }
            let Some(close) = matching_rparen(tokens, open) else {
                break;
            };
            let Some(comma) = next_significant(tokens, close + 1, tokens.len()) else {
                break;
            };
            if tokens[comma].token != Token::Comma {
                break;
            }
            let Some(next_name) = next_significant(tokens, comma + 1, tokens.len()) else {
                break;
            };
            if !is_identifier(&tokens[next_name]) {
                break;
            }
            name = next_name;
        }
    }
    Ok(())
}

fn validate_fetch_clauses(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for fetch in 0..tokens.len() {
        if !is_unquoted_word(&tokens[fetch], "fetch") {
            continue;
        }
        let invalid = || syntax_error(&tokens[fetch], "invalid Trino FETCH clause");
        let Some(mut cursor) = next_significant(tokens, fetch + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[cursor], "first") && !is_unquoted_word(&tokens[cursor], "next")
        {
            continue;
        }
        cursor = next_significant(tokens, cursor + 1, tokens.len()).ok_or_else(invalid)?;
        let is_count = matches!(&tokens[cursor].token, Token::Number(value, false) if !value.contains(['.', 'e', 'E']))
            || matches!(&tokens[cursor].token, Token::Placeholder(value) if value == "?");
        if is_count {
            cursor = next_significant(tokens, cursor + 1, tokens.len()).ok_or_else(invalid)?;
        }
        if !is_unquoted_word(&tokens[cursor], "row") && !is_unquoted_word(&tokens[cursor], "rows") {
            return Err(syntax_error(
                &tokens[cursor],
                "Trino FETCH requires ROW or ROWS",
            ));
        }
        cursor = next_significant(tokens, cursor + 1, tokens.len()).ok_or_else(invalid)?;
        if is_unquoted_word(&tokens[cursor], "only") {
            continue;
        }
        if !is_unquoted_word(&tokens[cursor], "with") {
            return Err(syntax_error(
                &tokens[cursor],
                "Trino FETCH requires ONLY or WITH TIES",
            ));
        }
        let ties = next_significant(tokens, cursor + 1, tokens.len()).ok_or_else(invalid)?;
        if !is_unquoted_word(&tokens[ties], "ties") {
            return Err(syntax_error(
                &tokens[ties],
                "Trino FETCH requires WITH TIES",
            ));
        }
    }
    Ok(())
}

fn validate_json_value_invocations(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for function in 0..tokens.len() {
        if !is_unquoted_word(&tokens[function], "json_value") {
            continue;
        }
        if previous_significant(tokens, function)
            .is_some_and(|previous| tokens[previous].token == Token::Period)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, function + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let mut depth = 0usize;
        let comma = (open + 1..close).find(|index| match tokens[*index].token {
            Token::LParen | Token::LBracket | Token::LBrace => {
                depth += 1;
                false
            }
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.saturating_sub(1);
                false
            }
            Token::Comma if depth == 0 => true,
            _ => false,
        });
        let Some(comma) = comma else {
            return Err(syntax_error(
                &tokens[open],
                "JSON_VALUE requires a JSON path argument",
            ));
        };
        let Some(path) = next_significant(tokens, comma + 1, close) else {
            return Err(syntax_error(
                &tokens[comma],
                "JSON_VALUE requires a JSON path string",
            ));
        };
        if !matches!(
            tokens[path].token,
            Token::SingleQuotedString(_) | Token::UnicodeStringLiteral(_)
        ) {
            return Err(syntax_error(
                &tokens[path],
                "JSON_VALUE requires a JSON path string",
            ));
        }
    }
    Ok(())
}

fn validate_with_ordinality(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for ordinality in 0..tokens.len() {
        if !is_unquoted_word(&tokens[ordinality], "ordinality") {
            continue;
        }
        let Some(with) = previous_significant(tokens, ordinality)
            .filter(|with| is_unquoted_word(&tokens[*with], "with"))
        else {
            continue;
        };
        let valid = previous_significant(tokens, with)
            .filter(|close| tokens[*close].token == Token::RParen)
            .and_then(|close| matching_lparen(tokens, close))
            .and_then(|open| previous_significant(tokens, open))
            .is_some_and(|function| is_unquoted_word(&tokens[function], "unnest"));
        if !valid {
            return Err(syntax_error(
                &tokens[ordinality],
                "WITH ORDINALITY is only valid after UNNEST",
            ));
        }
    }
    Ok(())
}

fn validate_explain_options(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for explain in 0..tokens.len() {
        if !is_unquoted_word(&tokens[explain], "explain") {
            continue;
        }
        let start = statement_start(tokens, explain);
        if next_significant(tokens, start, tokens.len()) != Some(explain) {
            continue;
        }
        let Some(open) = next_significant(tokens, explain + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        for option in open + 1..close {
            if !is_unquoted_word(&tokens[option], "type") {
                continue;
            }
            let Some(value) = next_significant(tokens, option + 1, close) else {
                return Err(syntax_error(
                    &tokens[option],
                    "EXPLAIN TYPE requires a value",
                ));
            };
            if !["logical", "distributed", "validate", "io"]
                .iter()
                .any(|word| is_unquoted_word(&tokens[value], word))
            {
                return Err(syntax_error(&tokens[value], "invalid Trino EXPLAIN TYPE"));
            }
        }
    }
    Ok(())
}

fn validate_show_statements(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    let allowed = [
        "branches",
        "catalogs",
        "columns",
        "create",
        "current",
        "functions",
        "grants",
        "role",
        "roles",
        "schemas",
        "session",
        "stats",
        "tables",
    ];
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        if let Some(show) = next_significant(tokens, start, end) {
            if is_unquoted_word(&tokens[show], "show") {
                let Some(object) = next_significant(tokens, show + 1, end) else {
                    return Err(syntax_error(&tokens[show], "SHOW requires an object"));
                };
                if !allowed
                    .iter()
                    .any(|word| is_unquoted_word(&tokens[object], word))
                {
                    return Err(syntax_error(&tokens[object], "unsupported Trino SHOW form"));
                }
            }
        }
        start = end.saturating_add(1);
    }
    Ok(())
}

fn validate_current_values(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for current in 0..tokens.len() {
        let name = [
            "current_date",
            "current_time",
            "current_timestamp",
            "localtime",
            "localtimestamp",
        ]
        .iter()
        .find(|name| is_unquoted_word(&tokens[current], name));
        let Some(name) = name else {
            continue;
        };
        if previous_significant(tokens, current)
            .is_some_and(|previous| tokens[previous].token == Token::Period)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, current + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        if *name == "current_date" {
            return Err(syntax_error(
                &tokens[open],
                "CURRENT_DATE does not accept a precision",
            ));
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let Some(precision) = next_significant(tokens, open + 1, close) else {
            return Err(syntax_error(
                &tokens[open],
                "current time precision requires one integer",
            ));
        };
        if !matches!(&tokens[precision].token, Token::Number(value, false) if !value.contains(['.', 'e', 'E']))
            || next_significant(tokens, precision + 1, close).is_some()
        {
            return Err(syntax_error(
                &tokens[precision],
                "current time precision requires one integer",
            ));
        }
    }
    Ok(())
}

fn validate_interval_qualifiers(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for interval in 0..tokens.len() {
        if !is_unquoted_word(&tokens[interval], "interval") {
            continue;
        }
        let Some(value) = next_significant(tokens, interval + 1, tokens.len()) else {
            continue;
        };
        if !is_single_quoted_string(&tokens[value]) {
            continue;
        }
        let Some(unit) = next_significant(tokens, value + 1, tokens.len()) else {
            continue;
        };
        if is_unquoted_word(&tokens[unit], "to") {
            return Err(syntax_error(
                &tokens[unit],
                "INTERVAL TO requires a starting field",
            ));
        }
    }
    Ok(())
}

fn validate_execute_shapes(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(execute) = next_significant(tokens, start, end) else {
            break;
        };
        if is_unquoted_word(&tokens[execute], "execute") {
            let Some(target) = next_significant(tokens, execute + 1, end) else {
                return Err(syntax_error(&tokens[execute], "EXECUTE requires a target"));
            };
            if is_unquoted_word(&tokens[target], "immediate") {
                let Some(sql) = next_significant(tokens, target + 1, end) else {
                    return Err(syntax_error(
                        &tokens[target],
                        "EXECUTE IMMEDIATE requires a string",
                    ));
                };
                if !matches!(
                    tokens[sql].token,
                    Token::SingleQuotedString(_) | Token::UnicodeStringLiteral(_)
                ) {
                    return Err(syntax_error(
                        &tokens[sql],
                        "EXECUTE IMMEDIATE requires a string",
                    ));
                }
            } else if is_unquoted_word(&tokens[target], "using") || !is_identifier(&tokens[target])
            {
                return Err(syntax_error(
                    &tokens[target],
                    "EXECUTE requires a prepared statement name",
                ));
            }
        }
        start = end.saturating_add(1);
    }
    Ok(())
}

fn validate_trino_source_shapes(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    validate_cte_as(tokens)?;
    validate_fetch_clauses(tokens)?;
    validate_json_value_invocations(tokens)?;
    validate_with_ordinality(tokens)?;
    validate_explain_options(tokens)?;
    validate_show_statements(tokens)?;
    validate_current_values(tokens)?;
    validate_interval_qualifiers(tokens)?;
    validate_execute_shapes(tokens)?;
    Ok(())
}

fn validate_trino_typed_literals(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for index in 0..tokens.len() {
        if !["date", "time", "timestamp", "interval", "double"]
            .iter()
            .any(|word| is_unquoted_word(&tokens[index], word))
        {
            continue;
        }
        let Some(value) = next_significant(tokens, index + 1, tokens.len()) else {
            continue;
        };
        if matches!(tokens[value].token, Token::Number(_, _)) {
            return Err(syntax_error(
                &tokens[value],
                "Trino typed literals require a quoted value",
            ));
        }
    }
    Ok(())
}

fn has_relation_introducer_before(tokens: &[TokenWithSpan], before: usize) -> bool {
    let mut depth = 0usize;
    for index in (0..before).rev() {
        match tokens[index].token {
            Token::Whitespace(_) => continue,
            Token::RParen | Token::RBracket | Token::RBrace => depth += 1,
            Token::LParen | Token::LBracket | Token::LBrace => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
            }
            _ if depth > 0 => continue,
            _ if is_unquoted_word(&tokens[index], "from")
                || is_unquoted_word(&tokens[index], "join") =>
            {
                return true
            }
            _ if [
                "where",
                "on",
                "group",
                "order",
                "having",
                "limit",
                "offset",
                "union",
                "intersect",
                "except",
                "select",
                "values",
            ]
            .iter()
            .any(|word| is_unquoted_word(&tokens[index], word)) =>
            {
                return false
            }
            _ => {}
        }
    }
    false
}

fn validate_trino_table_samples(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for sample in 0..tokens.len() {
        if !is_unquoted_word(&tokens[sample], "tablesample")
            || !has_relation_introducer_before(tokens, sample)
        {
            continue;
        }
        let method = next_significant(tokens, sample + 1, tokens.len());
        let valid_method = method.is_some_and(|method| {
            is_unquoted_word(&tokens[method], "bernoulli")
                || is_unquoted_word(&tokens[method], "system")
        });
        if !valid_method {
            continue;
        }
        let method = method.expect("checked as present");
        let Some(open) = next_significant(tokens, method + 1, tokens.len()) else {
            return Err(syntax_error(
                &tokens[method],
                "TABLESAMPLE requires a percentage expression",
            ));
        };
        if tokens[open].token != Token::LParen || matching_rparen(tokens, open).is_none() {
            return Err(syntax_error(
                &tokens[open],
                "TABLESAMPLE requires a parenthesized percentage expression",
            ));
        }
    }
    Ok(())
}

fn validate_trino_create_table_forms(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(create) = next_significant(tokens, start, end) else {
            break;
        };
        if !is_unquoted_word(&tokens[create], "create") {
            start = end.saturating_add(1);
            continue;
        }
        let mut cursor = next_significant(tokens, create + 1, end);
        let mut or_replace = false;
        if cursor.is_some_and(|index| is_unquoted_word(&tokens[index], "or")) {
            or_replace = true;
            cursor = cursor.and_then(|index| next_significant(tokens, index + 1, end));
            if !cursor.is_some_and(|index| is_unquoted_word(&tokens[index], "replace")) {
                start = end.saturating_add(1);
                continue;
            }
            cursor = cursor.and_then(|index| next_significant(tokens, index + 1, end));
        }
        let Some(table) = cursor else {
            start = end.saturating_add(1);
            continue;
        };
        if !is_unquoted_word(&tokens[table], "table") {
            start = end.saturating_add(1);
            continue;
        }
        let mut name = next_significant(tokens, table + 1, end);
        if name.is_some_and(|index| is_unquoted_word(&tokens[index], "if")) {
            if or_replace {
                let if_index = name.expect("checked as present");
                return Err(syntax_error(
                    &tokens[if_index],
                    "CREATE TABLE cannot combine OR REPLACE with IF NOT EXISTS",
                ));
            }
            let not = name.and_then(|index| next_significant(tokens, index + 1, end));
            let exists = not.and_then(|index| next_significant(tokens, index + 1, end));
            if !not.is_some_and(|index| is_unquoted_word(&tokens[index], "not"))
                || !exists.is_some_and(|index| is_unquoted_word(&tokens[index], "exists"))
            {
                start = end.saturating_add(1);
                continue;
            }
            name = exists.and_then(|index| next_significant(tokens, index + 1, end));
        }
        let Some(name) = name else {
            return Err(syntax_error(&tokens[table], "CREATE TABLE requires a name"));
        };
        let Some(after_name) = consume_qualified_name(tokens, name, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let definition = next_significant(tokens, after_name, end);
        let valid_definition = match definition {
            Some(index) if tokens[index].token == Token::LParen => matching_rparen(tokens, index)
                .is_some_and(|close| next_significant(tokens, index + 1, close).is_some()),
            Some(index) if is_unquoted_word(&tokens[index], "like") => true,
            Some(_) => create_table_as_prefix(tokens, after_name, end)?.is_some(),
            None => false,
        };
        if !valid_definition {
            return Err(syntax_error(
                &tokens[name],
                "CREATE TABLE requires columns, LIKE, or AS",
            ));
        }
        start = end.saturating_add(1);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct CreateTableAsPrefix {
    comment: Option<(usize, usize)>,
}

fn create_table_as_prefix(
    tokens: &[TokenWithSpan],
    name_end: usize,
    end: usize,
) -> Result<Option<CreateTableAsPrefix>, ParserError> {
    let Some(mut cursor) = next_significant(tokens, name_end, end) else {
        return Ok(None);
    };
    if !(is_unquoted_word(&tokens[cursor], "comment")
        || is_unquoted_word(&tokens[cursor], "with")
        || is_unquoted_word(&tokens[cursor], "as"))
    {
        return Ok(None);
    }
    let mut comment = None;
    if is_unquoted_word(&tokens[cursor], "comment") {
        let value = next_significant(tokens, cursor + 1, end)
            .ok_or_else(|| syntax_error(&tokens[cursor], "TABLE COMMENT requires a string"))?;
        if !is_single_quoted_string(&tokens[value]) {
            return Err(syntax_error(
                &tokens[value],
                "TABLE COMMENT requires a string",
            ));
        }
        comment = Some((cursor, value + 1));
        cursor = next_significant(tokens, value + 1, end).ok_or_else(|| {
            syntax_error(&tokens[value], "CREATE TABLE AS requires AS and a query")
        })?;
    }
    if is_unquoted_word(&tokens[cursor], "with") {
        let open = next_significant(tokens, cursor + 1, end)
            .ok_or_else(|| syntax_error(&tokens[cursor], "TABLE WITH requires properties"))?;
        if tokens[open].token != Token::LParen {
            return Err(syntax_error(&tokens[open], "TABLE WITH requires '('"));
        }
        let close = matching_rparen(tokens, open)
            .ok_or_else(|| syntax_error(&tokens[open], "unterminated table properties"))?;
        if next_significant(tokens, open + 1, close).is_none() {
            return Err(syntax_error(
                &tokens[open],
                "TABLE WITH requires properties",
            ));
        }
        cursor = next_significant(tokens, close + 1, end).ok_or_else(|| {
            syntax_error(&tokens[close], "CREATE TABLE AS requires AS and a query")
        })?;
    }
    if !is_unquoted_word(&tokens[cursor], "as") {
        return Err(syntax_error(
            &tokens[cursor],
            "CREATE TABLE options must be followed by AS",
        ));
    }
    next_significant(tokens, cursor + 1, end)
        .ok_or_else(|| syntax_error(&tokens[cursor], "CREATE TABLE AS requires a query"))?;
    Ok(Some(CreateTableAsPrefix { comment }))
}

fn create_table_name_end(tokens: &[TokenWithSpan], start: usize, end: usize) -> Option<usize> {
    let create = next_significant(tokens, start, end)?;
    if !is_unquoted_word(&tokens[create], "create") {
        return None;
    }
    let mut cursor = next_significant(tokens, create + 1, end)?;
    if is_unquoted_word(&tokens[cursor], "or") {
        let replace = next_significant(tokens, cursor + 1, end)?;
        if !is_unquoted_word(&tokens[replace], "replace") {
            return None;
        }
        cursor = next_significant(tokens, replace + 1, end)?;
    }
    if !is_unquoted_word(&tokens[cursor], "table") {
        return None;
    }
    cursor = next_significant(tokens, cursor + 1, end)?;
    if is_unquoted_word(&tokens[cursor], "if") {
        let not = next_significant(tokens, cursor + 1, end)?;
        let exists = next_significant(tokens, not + 1, end)?;
        if !is_unquoted_word(&tokens[not], "not") || !is_unquoted_word(&tokens[exists], "exists") {
            return None;
        }
        cursor = next_significant(tokens, exists + 1, end)?;
    }
    consume_qualified_name(tokens, cursor, end)
}

fn top_level_ranges(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
) -> Result<Vec<(usize, usize)>, ParserError> {
    let mut ranges = Vec::new();
    let mut item_start = start;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| syntax_error(token, "unbalanced property expression"))?
            }
            Token::Comma if depth == 0 => {
                if next_significant(tokens, item_start, index).is_none() {
                    return Err(syntax_error(token, "empty item before comma"));
                }
                ranges.push((item_start, index));
                item_start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(syntax_error(
            &tokens[start],
            "unterminated property expression",
        ));
    }
    if next_significant(tokens, item_start, end).is_none() {
        return Err(syntax_error(
            &tokens[end.saturating_sub(1)],
            "empty item after comma",
        ));
    }
    ranges.push((item_start, end));
    Ok(ranges)
}

fn property_expression_metadata(
    tokens: &[TokenWithSpan],
    open: usize,
    close: usize,
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    for (start, end) in top_level_ranges(tokens, open + 1, close)? {
        if let Some(statement) = property_assignment_metadata(tokens, start, end, dialect)? {
            metadata.push(statement);
        }
    }
    Ok(metadata)
}

fn property_assignment_metadata(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> Result<Option<Statement>, ParserError> {
    let key = next_significant(tokens, start, end).ok_or_else(|| {
        syntax_error(&tokens[start.saturating_sub(1)], "property requires a name")
    })?;
    if !is_identifier(&tokens[key]) {
        return Err(syntax_error(&tokens[key], "invalid property name"));
    }
    let eq = next_significant(tokens, key + 1, end)
        .ok_or_else(|| syntax_error(&tokens[key], "property requires '='"))?;
    if tokens[eq].token != Token::Eq {
        return Err(syntax_error(&tokens[eq], "property requires '='"));
    }
    let value = next_significant(tokens, eq + 1, end)
        .ok_or_else(|| syntax_error(&tokens[eq], "property requires a value"))?;
    if is_unquoted_word(&tokens[value], "default")
        && next_significant(tokens, value + 1, end).is_none()
    {
        return Ok(None);
    }
    parse_expression_metadata(tokens, value, end, dialect).map(Some)
}

fn find_top_level_word(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    expected: &str,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.checked_sub(1)?;
            }
            _ if depth == 0 && is_unquoted_word(token, expected) => return Some(index),
            _ => {}
        }
    }
    None
}

fn normalize_create_table_extensions(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    let mut statement_start = 0usize;
    while statement_start < tokens.len() {
        let end = statement_end(tokens, statement_start);
        let Some(name_end) = create_table_name_end(tokens, statement_start, end) else {
            statement_start = end.saturating_add(1);
            continue;
        };
        let Some(definition) = next_significant(tokens, name_end, end) else {
            statement_start = end.saturating_add(1);
            continue;
        };
        if tokens[definition].token == Token::LParen {
            let close = matching_rparen(tokens, definition).ok_or_else(|| {
                syntax_error(&tokens[definition], "unterminated CREATE TABLE definition")
            })?;
            let as_index = find_top_level_word(tokens, close + 1, end, "as");
            if as_index.is_some() && column_aliases_are_valid(tokens, definition, close) {
                blank_non_whitespace(tokens, definition, close + 1);
            } else {
                for (element_start, element_end) in top_level_ranges(tokens, definition + 1, close)?
                {
                    let first = next_significant(tokens, element_start, element_end)
                        .ok_or_else(|| syntax_error(&tokens[definition], "empty table element"))?;
                    if is_unquoted_word(&tokens[first], "like") {
                        let name =
                            next_significant(tokens, first + 1, element_end).ok_or_else(|| {
                                syntax_error(&tokens[first], "LIKE requires a table name")
                            })?;
                        let after_name = consume_qualified_name(tokens, name, element_end)
                            .ok_or_else(|| {
                                syntax_error(&tokens[name], "invalid LIKE table name")
                            })?;
                        let type_name = previous_significant(tokens, after_name)
                            .expect("qualified name contains an identifier");
                        blank_non_whitespace(tokens, name, type_name);
                        replace_word(&mut tokens[type_name], "VARCHAR", Keyword::VARCHAR);
                        if let Some(modifier) = next_significant(tokens, after_name, element_end) {
                            if !(is_unquoted_word(&tokens[modifier], "including")
                                || is_unquoted_word(&tokens[modifier], "excluding"))
                            {
                                return Err(syntax_error(
                                    &tokens[modifier],
                                    "LIKE accepts INCLUDING or EXCLUDING PROPERTIES",
                                ));
                            }
                            let properties = next_significant(tokens, modifier + 1, element_end)
                                .ok_or_else(|| {
                                    syntax_error(
                                        &tokens[modifier],
                                        "LIKE modifier requires PROPERTIES",
                                    )
                                })?;
                            if !is_unquoted_word(&tokens[properties], "properties")
                                || next_significant(tokens, properties + 1, element_end).is_some()
                            {
                                return Err(syntax_error(
                                    &tokens[properties],
                                    "LIKE modifier requires PROPERTIES",
                                ));
                            }
                            blank_non_whitespace(tokens, modifier, element_end);
                        }
                        continue;
                    }
                    let Some(mut with) =
                        find_top_level_word(tokens, first + 1, element_end, "with")
                    else {
                        continue;
                    };
                    let after_with = next_significant(tokens, with + 1, element_end);
                    if after_with.is_some_and(|index| is_unquoted_word(&tokens[index], "time")) {
                        let zone = after_with
                            .and_then(|index| next_significant(tokens, index + 1, element_end));
                        if zone.is_some_and(|index| is_unquoted_word(&tokens[index], "zone")) {
                            let Some(properties_with) = zone.and_then(|index| {
                                find_top_level_word(tokens, index + 1, element_end, "with")
                            }) else {
                                continue;
                            };
                            with = properties_with;
                        }
                    }
                    let open =
                        next_significant(tokens, with + 1, element_end).ok_or_else(|| {
                            syntax_error(&tokens[with], "column WITH requires properties")
                        })?;
                    if tokens[open].token != Token::LParen {
                        return Err(syntax_error(&tokens[open], "column WITH requires '('"));
                    }
                    let properties_close = matching_rparen(tokens, open).ok_or_else(|| {
                        syntax_error(&tokens[open], "unterminated column properties")
                    })?;
                    if properties_close >= element_end
                        || next_significant(tokens, properties_close + 1, element_end).is_some()
                    {
                        return Err(syntax_error(
                            &tokens[properties_close],
                            "unexpected token after column properties",
                        ));
                    }
                    metadata.extend(property_expression_metadata(
                        tokens,
                        open,
                        properties_close,
                        dialect,
                    )?);
                    blank_non_whitespace(tokens, with, properties_close + 1);
                }
            }
            if let Some(comment) = next_significant(tokens, close + 1, end)
                .filter(|index| is_unquoted_word(&tokens[*index], "comment"))
            {
                let value = next_significant(tokens, comment + 1, end).ok_or_else(|| {
                    syntax_error(&tokens[comment], "TABLE COMMENT requires a string")
                })?;
                if !is_single_quoted_string(&tokens[value]) {
                    return Err(syntax_error(
                        &tokens[value],
                        "TABLE COMMENT requires a string",
                    ));
                }
                blank_non_whitespace(tokens, comment, value + 1);
            }
        } else if let Some(prefix) = create_table_as_prefix(tokens, name_end, end)? {
            if let Some((start, end)) = prefix.comment {
                blank_non_whitespace(tokens, start, end);
            }
        }
        let last = previous_significant(tokens, end);
        if let Some(data) = last.filter(|index| is_unquoted_word(&tokens[*index], "data")) {
            let previous = previous_significant(tokens, data);
            let with = previous.and_then(|index| {
                if is_unquoted_word(&tokens[index], "no") {
                    previous_significant(tokens, index)
                } else {
                    Some(index)
                }
            });
            if let Some(with) = with.filter(|index| is_unquoted_word(&tokens[*index], "with")) {
                blank_non_whitespace(tokens, with, data + 1);
            }
        }
        statement_start = end.saturating_add(1);
    }
    Ok(metadata)
}

fn normalize_analyze_properties(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(analyze) = next_significant(tokens, start, end) else {
            break;
        };
        if !is_unquoted_word(&tokens[analyze], "analyze") {
            start = end.saturating_add(1);
            continue;
        }
        let Some(name) = next_significant(tokens, analyze + 1, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let Some(name_end) = consume_qualified_name(tokens, name, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let Some(with) = next_significant(tokens, name_end, end) else {
            start = end.saturating_add(1);
            continue;
        };
        if !is_unquoted_word(&tokens[with], "with") {
            start = end.saturating_add(1);
            continue;
        }
        let open = next_significant(tokens, with + 1, end)
            .ok_or_else(|| syntax_error(&tokens[with], "ANALYZE WITH requires properties"))?;
        if tokens[open].token != Token::LParen {
            return Err(syntax_error(&tokens[open], "ANALYZE WITH requires '('"));
        }
        let close = matching_rparen(tokens, open)
            .ok_or_else(|| syntax_error(&tokens[open], "unterminated ANALYZE properties"))?;
        if next_significant(tokens, close + 1, end).is_some() {
            return Err(syntax_error(
                &tokens[close],
                "unexpected token after ANALYZE properties",
            ));
        }
        metadata.extend(property_expression_metadata(tokens, open, close, dialect)?);
        blank_non_whitespace(tokens, with, close + 1);
        start = end.saturating_add(1);
    }
    Ok(metadata)
}

fn normalize_create_view_options(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(create) = next_significant(tokens, start, end) else {
            break;
        };
        if !is_unquoted_word(&tokens[create], "create") {
            start = end.saturating_add(1);
            continue;
        }
        let mut cursor = next_significant(tokens, create + 1, end);
        if cursor.is_some_and(|index| is_unquoted_word(&tokens[index], "or")) {
            let replace = cursor.and_then(|index| next_significant(tokens, index + 1, end));
            if !replace.is_some_and(|index| is_unquoted_word(&tokens[index], "replace")) {
                start = end.saturating_add(1);
                continue;
            }
            cursor = replace.and_then(|index| next_significant(tokens, index + 1, end));
        }
        let Some(view) = cursor.filter(|index| is_unquoted_word(&tokens[*index], "view")) else {
            start = end.saturating_add(1);
            continue;
        };
        let Some(name) = next_significant(tokens, view + 1, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let Some(mut cursor) = consume_qualified_name(tokens, name, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let mut ranges = Vec::new();
        if let Some(comment) = next_significant(tokens, cursor, end)
            .filter(|index| is_unquoted_word(&tokens[*index], "comment"))
        {
            let value = next_significant(tokens, comment + 1, end)
                .ok_or_else(|| syntax_error(&tokens[comment], "VIEW COMMENT requires a string"))?;
            if !is_single_quoted_string(&tokens[value]) {
                return Err(syntax_error(
                    &tokens[value],
                    "VIEW COMMENT requires a string",
                ));
            }
            cursor = value + 1;
            ranges.push((comment, cursor));
        }
        if let Some(security) = next_significant(tokens, cursor, end)
            .filter(|index| is_unquoted_word(&tokens[*index], "security"))
        {
            let mode = next_significant(tokens, security + 1, end)
                .ok_or_else(|| syntax_error(&tokens[security], "VIEW SECURITY requires a mode"))?;
            if !(is_unquoted_word(&tokens[mode], "definer")
                || is_unquoted_word(&tokens[mode], "invoker"))
            {
                return Err(syntax_error(
                    &tokens[mode],
                    "VIEW SECURITY requires DEFINER or INVOKER",
                ));
            }
            cursor = mode + 1;
            ranges.push((security, cursor));
        }
        if let Some(with) = next_significant(tokens, cursor, end)
            .filter(|index| is_unquoted_word(&tokens[*index], "with"))
        {
            let open = next_significant(tokens, with + 1, end)
                .ok_or_else(|| syntax_error(&tokens[with], "VIEW WITH requires properties"))?;
            if tokens[open].token != Token::LParen {
                return Err(syntax_error(&tokens[open], "VIEW WITH requires '('"));
            }
            let close = matching_rparen(tokens, open)
                .ok_or_else(|| syntax_error(&tokens[open], "unterminated VIEW properties"))?;
            cursor = close + 1;
        }
        let Some(as_index) = next_significant(tokens, cursor, end) else {
            start = end.saturating_add(1);
            continue;
        };
        if !is_unquoted_word(&tokens[as_index], "as") || ranges.is_empty() {
            start = end.saturating_add(1);
            continue;
        }
        for (range_start, range_end) in ranges {
            blank_non_whitespace(tokens, range_start, range_end);
        }
        start = end.saturating_add(1);
    }
    Ok(())
}

fn normalize_json_table_scalar_columns(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for json_table in 0..tokens.len() {
        if !is_unquoted_word(&tokens[json_table], "json_table") {
            continue;
        }
        let Some(open) = next_significant(tokens, json_table + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(table_close) = matching_rparen(tokens, open) else {
            continue;
        };
        let Some(columns) = find_top_level_word(tokens, open + 1, table_close, "columns") else {
            continue;
        };
        let columns_open = next_significant(tokens, columns + 1, table_close)
            .ok_or_else(|| syntax_error(&tokens[columns], "JSON_TABLE COLUMNS requires '('"))?;
        if tokens[columns_open].token != Token::LParen {
            return Err(syntax_error(
                &tokens[columns_open],
                "JSON_TABLE COLUMNS requires '('",
            ));
        }
        let columns_close = matching_rparen(tokens, columns_open).ok_or_else(|| {
            syntax_error(&tokens[columns_open], "unterminated JSON_TABLE COLUMNS")
        })?;
        for (column_start, column_end) in top_level_ranges(tokens, columns_open + 1, columns_close)?
        {
            if let Some(format) = find_top_level_word(tokens, column_start, column_end, "format") {
                let json = next_significant(tokens, format + 1, column_end)
                    .ok_or_else(|| syntax_error(&tokens[format], "FORMAT requires JSON"))?;
                if !is_unquoted_word(&tokens[json], "json") {
                    return Err(syntax_error(&tokens[json], "FORMAT requires JSON"));
                }
                let mut format_end = json + 1;
                if let Some(encoding) = next_significant(tokens, format_end, column_end)
                    .filter(|index| is_unquoted_word(&tokens[*index], "encoding"))
                {
                    let value =
                        next_significant(tokens, encoding + 1, column_end).ok_or_else(|| {
                            syntax_error(
                                &tokens[encoding],
                                "ENCODING requires UTF8, UTF16, or UTF32",
                            )
                        })?;
                    if !["utf8", "utf16", "utf32"]
                        .iter()
                        .any(|name| is_unquoted_word(&tokens[value], name))
                    {
                        return Err(syntax_error(
                            &tokens[value],
                            "ENCODING requires UTF8, UTF16, or UTF32",
                        ));
                    }
                    format_end = value + 1;
                }
                blank_non_whitespace(tokens, format, format_end);
            }
            if let Some(wrapper) = find_top_level_word(tokens, column_start, column_end, "with")
                .or_else(|| find_top_level_word(tokens, column_start, column_end, "without"))
            {
                let mut cursor =
                    next_significant(tokens, wrapper + 1, column_end).ok_or_else(|| {
                        syntax_error(&tokens[wrapper], "incomplete JSON wrapper clause")
                    })?;
                if is_unquoted_word(&tokens[cursor], "conditional")
                    || is_unquoted_word(&tokens[cursor], "unconditional")
                {
                    cursor = next_significant(tokens, cursor + 1, column_end).ok_or_else(|| {
                        syntax_error(&tokens[wrapper], "incomplete JSON wrapper clause")
                    })?;
                }
                if is_unquoted_word(&tokens[cursor], "array") {
                    cursor = next_significant(tokens, cursor + 1, column_end).ok_or_else(|| {
                        syntax_error(&tokens[wrapper], "incomplete JSON wrapper clause")
                    })?;
                }
                if !is_unquoted_word(&tokens[cursor], "wrapper") {
                    return Err(syntax_error(
                        &tokens[cursor],
                        "wrapper clause requires WRAPPER",
                    ));
                }
                let mut wrapper_end = cursor + 1;
                if let Some(quotes) =
                    next_significant(tokens, wrapper_end, column_end).filter(|index| {
                        is_unquoted_word(&tokens[*index], "keep")
                            || is_unquoted_word(&tokens[*index], "omit")
                    })
                {
                    let keyword =
                        next_significant(tokens, quotes + 1, column_end).ok_or_else(|| {
                            syntax_error(&tokens[quotes], "KEEP/OMIT requires QUOTES")
                        })?;
                    if !is_unquoted_word(&tokens[keyword], "quotes") {
                        return Err(syntax_error(&tokens[keyword], "KEEP/OMIT requires QUOTES"));
                    }
                    wrapper_end = keyword + 1;
                }
                blank_non_whitespace(tokens, wrapper, wrapper_end);
            }
            let mut cursor = column_start;
            while let Some(empty) = find_top_level_word(tokens, cursor, column_end, "empty") {
                let Some(container) = next_significant(tokens, empty + 1, column_end) else {
                    break;
                };
                if !(is_unquoted_word(&tokens[container], "array")
                    || is_unquoted_word(&tokens[container], "object"))
                {
                    cursor = container;
                    continue;
                }
                let on = next_significant(tokens, container + 1, column_end)
                    .ok_or_else(|| syntax_error(&tokens[container], "EMPTY value requires ON"))?;
                let event = next_significant(tokens, on + 1, column_end)
                    .ok_or_else(|| syntax_error(&tokens[on], "EMPTY value requires an event"))?;
                if !is_unquoted_word(&tokens[on], "on")
                    || !(is_unquoted_word(&tokens[event], "empty")
                        || is_unquoted_word(&tokens[event], "error"))
                {
                    return Err(syntax_error(
                        &tokens[on],
                        "EMPTY ARRAY/OBJECT requires ON EMPTY or ON ERROR",
                    ));
                }
                replace_word(&mut tokens[empty], "NULL", Keyword::NULL);
                blank_non_whitespace(tokens, container, on);
                cursor = event + 1;
            }
        }
        if let Some(empty) = next_significant(tokens, columns_close + 1, table_close) {
            let on = next_significant(tokens, empty + 1, table_close).ok_or_else(|| {
                syntax_error(&tokens[empty], "JSON_TABLE EMPTY requires ON ERROR")
            })?;
            let error = next_significant(tokens, on + 1, table_close)
                .ok_or_else(|| syntax_error(&tokens[on], "JSON_TABLE EMPTY requires ON ERROR"))?;
            if !is_unquoted_word(&tokens[empty], "empty")
                || !is_unquoted_word(&tokens[on], "on")
                || !is_unquoted_word(&tokens[error], "error")
                || next_significant(tokens, error + 1, table_close).is_some()
            {
                return Err(syntax_error(
                    &tokens[empty],
                    "unsupported JSON_TABLE error behavior",
                ));
            }
            blank_non_whitespace(tokens, empty, error + 1);
        }
    }
    Ok(())
}

fn normalize_json_object_pairs(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for json_object in 0..tokens.len() {
        if !is_unquoted_word(&tokens[json_object], "json_object") {
            continue;
        }
        let Some(open) = next_significant(tokens, json_object + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        if next_significant(tokens, open + 1, close).is_none() {
            continue;
        }
        for (start, end) in top_level_ranges(tokens, open + 1, close)? {
            let Some(first) = next_significant(tokens, start, end) else {
                continue;
            };
            if (is_unquoted_word(&tokens[first], "null")
                || is_unquoted_word(&tokens[first], "absent")
                || is_unquoted_word(&tokens[first], "returning"))
                && !tokens[start..end]
                    .iter()
                    .any(|token| token.token == Token::Colon || is_unquoted_word(token, "value"))
            {
                continue;
            }
            let key = if is_unquoted_word(&tokens[first], "key") {
                let key = next_significant(tokens, first + 1, end).ok_or_else(|| {
                    syntax_error(&tokens[first], "JSON_OBJECT KEY requires a value")
                })?;
                blank_non_whitespace(tokens, first, key);
                key
            } else {
                first
            };
            let separators: Vec<usize> = tokens
                .iter()
                .enumerate()
                .take(end)
                .skip(key)
                .scan(0usize, |depth, (index, token)| {
                    let at_top_level = *depth == 0;
                    match token.token {
                        Token::LParen | Token::LBracket | Token::LBrace => *depth += 1,
                        Token::RParen | Token::RBracket | Token::RBrace if *depth > 0 => {
                            *depth -= 1
                        }
                        _ => {}
                    }
                    Some((index, token, at_top_level))
                })
                .filter_map(|(index, token, at_top_level)| {
                    (at_top_level
                        && (token.token == Token::Colon || is_unquoted_word(token, "value")))
                    .then_some(index)
                })
                .collect();
            if separators.len() != 1 {
                return Err(syntax_error(
                    &tokens[key],
                    "JSON_OBJECT entry requires exactly one key/value separator",
                ));
            }
            let separator = separators[0];
            if next_significant(tokens, key, separator).is_none()
                || next_significant(tokens, separator + 1, end).is_none()
            {
                return Err(syntax_error(
                    &tokens[separator],
                    "JSON_OBJECT entry requires a key and value",
                ));
            }
            tokens[separator].token = Token::Comma;
        }
    }
    Ok(())
}

fn normalize_json_query_wrappers(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for json_query in 0..tokens.len() {
        if !is_unquoted_word(&tokens[json_query], "json_query") {
            continue;
        }
        let Some(open) = next_significant(tokens, json_query + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        if let Some(wrapper) = find_top_level_word(tokens, open + 1, close, "with")
            .or_else(|| find_top_level_word(tokens, open + 1, close, "without"))
        {
            let mut cursor = next_significant(tokens, wrapper + 1, close)
                .ok_or_else(|| syntax_error(&tokens[wrapper], "incomplete JSON wrapper clause"))?;
            if is_unquoted_word(&tokens[cursor], "conditional")
                || is_unquoted_word(&tokens[cursor], "unconditional")
            {
                cursor = next_significant(tokens, cursor + 1, close).ok_or_else(|| {
                    syntax_error(&tokens[wrapper], "incomplete JSON wrapper clause")
                })?;
            }
            if is_unquoted_word(&tokens[cursor], "array") {
                cursor = next_significant(tokens, cursor + 1, close).ok_or_else(|| {
                    syntax_error(&tokens[wrapper], "incomplete JSON wrapper clause")
                })?;
            }
            if !is_unquoted_word(&tokens[cursor], "wrapper") {
                return Err(syntax_error(
                    &tokens[cursor],
                    "wrapper clause requires WRAPPER",
                ));
            }
            blank_non_whitespace(tokens, wrapper, cursor + 1);
        }
        let mut quote_clause = None;
        let mut depth = 0usize;
        for quote in open + 1..close {
            match tokens[quote].token {
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    continue;
                }
                Token::RParen | Token::RBracket | Token::RBrace if depth > 0 => {
                    depth -= 1;
                    continue;
                }
                _ if depth > 0 => continue,
                _ => {}
            }
            if !is_unquoted_word(&tokens[quote], "keep")
                && !is_unquoted_word(&tokens[quote], "omit")
            {
                continue;
            }
            let Some(quotes) = next_significant(tokens, quote + 1, close) else {
                continue;
            };
            if !is_unquoted_word(&tokens[quotes], "quotes") {
                continue;
            }
            if quote_clause.is_some() {
                return Err(syntax_error(
                    &tokens[quote],
                    "JSON_QUERY accepts only one quotes clause",
                ));
            }
            let mut end = quotes + 1;
            if let Some(on) = next_significant(tokens, quotes + 1, close)
                .filter(|index| is_unquoted_word(&tokens[*index], "on"))
            {
                let scalar = next_significant(tokens, on + 1, close).ok_or_else(|| {
                    syntax_error(&tokens[on], "JSON_QUERY ON requires SCALAR STRING")
                })?;
                let string = next_significant(tokens, scalar + 1, close).ok_or_else(|| {
                    syntax_error(&tokens[scalar], "JSON_QUERY ON requires SCALAR STRING")
                })?;
                if !is_unquoted_word(&tokens[scalar], "scalar")
                    || !is_unquoted_word(&tokens[string], "string")
                {
                    return Err(syntax_error(
                        &tokens[scalar],
                        "JSON_QUERY ON requires SCALAR STRING",
                    ));
                }
                end = string + 1;
            }
            quote_clause = Some((quote, end));
        }
        if let Some((start, end)) = quote_clause {
            blank_non_whitespace(tokens, start, end);
        }
    }
    Ok(())
}

fn normalize_json_behaviors(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    for json_function in 0..tokens.len() {
        let is_json_value = is_unquoted_word(&tokens[json_function], "json_value");
        let is_json_query = is_unquoted_word(&tokens[json_function], "json_query");
        if !is_json_value && !is_json_query {
            continue;
        }
        let Some(open) = next_significant(tokens, json_function + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let mut depth = 0usize;
        let mut last_default = None;
        let mut ranges = Vec::new();
        let mut seen_empty = false;
        let mut seen_error = false;
        for on in open + 1..close {
            match tokens[on].token {
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    continue;
                }
                Token::RParen | Token::RBracket | Token::RBrace if depth > 0 => {
                    depth -= 1;
                    continue;
                }
                _ => {}
            }
            if depth == 0 && is_json_value && is_unquoted_word(&tokens[on], "default") {
                last_default = Some(on);
                continue;
            }
            if depth != 0 || !is_unquoted_word(&tokens[on], "on") {
                continue;
            }
            let Some(condition) = next_significant(tokens, on + 1, close) else {
                continue;
            };
            if !(is_unquoted_word(&tokens[condition], "empty")
                || is_unquoted_word(&tokens[condition], "error"))
            {
                continue;
            }
            let seen = if is_unquoted_word(&tokens[condition], "empty") {
                &mut seen_empty
            } else {
                &mut seen_error
            };
            if *seen {
                return Err(syntax_error(
                    &tokens[on],
                    "JSON behavior may be specified only once",
                ));
            }
            *seen = true;
            let Some(previous) = previous_significant(tokens, on) else {
                continue;
            };
            if is_unquoted_word(&tokens[previous], "null")
                || is_unquoted_word(&tokens[previous], "error")
            {
                ranges.push((previous, condition + 1));
                last_default = None;
                continue;
            }
            if is_json_query
                && (is_unquoted_word(&tokens[previous], "array")
                    || is_unquoted_word(&tokens[previous], "object"))
            {
                let Some(empty) = previous_significant(tokens, previous) else {
                    continue;
                };
                if is_unquoted_word(&tokens[empty], "empty") {
                    ranges.push((empty, condition + 1));
                    continue;
                }
            }
            let Some(default) = last_default.take() else {
                continue;
            };
            let expression = next_significant(tokens, default + 1, on).ok_or_else(|| {
                syntax_error(
                    &tokens[default],
                    "JSON_VALUE DEFAULT requires an expression",
                )
            })?;
            metadata.push(parse_expression_metadata(tokens, expression, on, dialect)?);
            ranges.push((default, condition + 1));
        }
        for (start, end) in ranges {
            blank_non_whitespace(tokens, start, end);
        }
    }
    Ok(metadata)
}

fn validate_trino_reserved_expression_starts(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for where_index in 0..tokens.len() {
        if !is_unquoted_word(&tokens[where_index], "where") {
            continue;
        }
        let Some(expression) = next_significant(tokens, where_index + 1, tokens.len()) else {
            continue;
        };
        if is_unquoted_word(&tokens[expression], "from") {
            return Err(syntax_error(
                &tokens[expression],
                "FROM cannot start a WHERE expression",
            ));
        }
    }
    Ok(())
}

fn validate_trino_count_distinct_wildcard(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for count in 0..tokens.len() {
        if !is_unquoted_word(&tokens[count], "count") {
            continue;
        }
        let Some(open) = next_significant(tokens, count + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(distinct) = next_significant(tokens, open + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[distinct], "distinct") {
            continue;
        }
        let Some(asterisk) = next_significant(tokens, distinct + 1, tokens.len()) else {
            continue;
        };
        if tokens[asterisk].token == Token::Mul {
            return Err(syntax_error(
                &tokens[asterisk],
                "COUNT does not allow DISTINCT *",
            ));
        }
    }
    Ok(())
}

fn create_materialized_view_keyword(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
) -> Option<usize> {
    let create = next_significant(tokens, start, end)?;
    if !is_unquoted_word(&tokens[create], "create") {
        return None;
    };
    let mut cursor = next_significant(tokens, create + 1, end)?;
    if is_unquoted_word(&tokens[cursor], "or") {
        let replace = next_significant(tokens, cursor + 1, end)?;
        if !is_unquoted_word(&tokens[replace], "replace") {
            return None;
        }
        cursor = next_significant(tokens, replace + 1, end)?;
    }
    if !is_unquoted_word(&tokens[cursor], "materialized") {
        return None;
    }
    let view = next_significant(tokens, cursor + 1, end)?;
    is_unquoted_word(&tokens[view], "view").then_some(view)
}

fn statement_end(tokens: &[TokenWithSpan], start: usize) -> usize {
    tokens[start..]
        .iter()
        .position(|token| token.token == Token::SemiColon)
        .map_or(tokens.len(), |offset| start + offset)
}

fn materialized_view_option_rank(token: &TokenWithSpan) -> Option<usize> {
    ["grace", "when", "comment", "with"]
        .iter()
        .position(|word| is_unquoted_word(token, word))
}

fn is_single_quoted_string(token: &TokenWithSpan) -> bool {
    matches!(
        token.token,
        Token::SingleQuotedString(_) | Token::UnicodeStringLiteral(_)
    )
}

fn is_interval_unit(token: &TokenWithSpan) -> bool {
    ["year", "month", "day", "hour", "minute", "second"]
        .iter()
        .any(|unit| is_unquoted_word(token, unit))
}

fn materialized_view_options(
    tokens: &[TokenWithSpan],
    view: usize,
    end: usize,
) -> Option<(usize, Vec<(usize, usize)>)> {
    let mut depth = 0usize;
    let mut clauses = Vec::new();
    let mut as_index = None;
    for (index, token) in tokens.iter().enumerate().take(end).skip(view + 1) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.checked_sub(1)?;
            }
            _ if depth == 0 && is_unquoted_word(token, "as") => {
                as_index = Some(index);
                break;
            }
            _ if depth == 0 => {
                if let Some(rank) = materialized_view_option_rank(token) {
                    clauses.push((index, rank));
                }
            }
            _ => {}
        }
    }
    let as_index = as_index?;
    if clauses.windows(2).any(|pair| pair[0].1 >= pair[1].1) {
        return None;
    }
    Some((as_index, clauses))
}

fn blank_non_whitespace(tokens: &mut [TokenWithSpan], start: usize, end: usize) {
    for token in &mut tokens[start..end] {
        if !matches!(token.token, Token::Whitespace(_)) {
            token.token = Token::Whitespace(Whitespace::Space);
        }
    }
}

fn word_with_span(token: &TokenWithSpan, value: &str, keyword: Keyword) -> TokenWithSpan {
    TokenWithSpan::new(
        Token::Word(Word {
            value: value.to_string(),
            quote_style: None,
            keyword,
        }),
        token.span,
    )
}

fn token_with_span(token: Token, source: &TokenWithSpan) -> TokenWithSpan {
    TokenWithSpan::new(token, source.span)
}

fn replace_word(token: &mut TokenWithSpan, value: &str, keyword: Keyword) {
    token.token = Token::Word(Word {
        value: value.to_string(),
        quote_style: None,
        keyword,
    });
}

fn normalize_prepare_from(tokens: &mut [TokenWithSpan]) {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(prepare) = next_significant(tokens, start, end) else {
            break;
        };
        if !is_unquoted_word(&tokens[prepare], "prepare") {
            start = end.saturating_add(1);
            continue;
        }
        let Some(name) = next_significant(tokens, prepare + 1, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let Some(from) = next_significant(tokens, name + 1, end) else {
            start = end.saturating_add(1);
            continue;
        };
        if is_identifier(&tokens[name]) && is_unquoted_word(&tokens[from], "from") {
            replace_word(&mut tokens[from], "AS", Keyword::AS);
        }
        start = end.saturating_add(1);
    }
}

fn normalize_array_parenthesis_types(tokens: &mut [TokenWithSpan]) {
    for array in 0..tokens.len() {
        if !is_unquoted_word(&tokens[array], "array") || !is_structural_type_position(tokens, array)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, array + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        tokens[open].token = Token::Lt;
        tokens[close].token = Token::Gt;
    }
}

fn normalize_table_queries(tokens: &mut [TokenWithSpan]) {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        if let Some(table) = next_significant(tokens, start, end) {
            if is_unquoted_word(&tokens[table], "table") {
                let relation = next_significant(tokens, table + 1, end);
                let after_name =
                    relation.and_then(|index| consume_qualified_name(tokens, index, end));
                let suffix = after_name.and_then(|index| next_significant(tokens, index, end));
                let valid_suffix = suffix.map_or(true, |index| {
                    [
                        "except",
                        "fetch",
                        "intersect",
                        "limit",
                        "offset",
                        "order",
                        "union",
                    ]
                    .iter()
                    .any(|word| is_unquoted_word(&tokens[index], word))
                });
                if after_name.is_some() && valid_suffix {
                    replace_word(&mut tokens[table], "SELECT", Keyword::SELECT);
                }
            }
        }
        start = end.saturating_add(1);
    }
}

fn normalize_top_identifiers(tokens: &mut [TokenWithSpan]) {
    for top in 0..tokens.len() {
        if !is_unquoted_word(&tokens[top], "top") {
            continue;
        }
        let previous = previous_significant(tokens, top);
        let next = next_significant(tokens, top + 1, tokens.len());
        if next.is_some_and(|index| tokens[index].token == Token::Period)
            || previous.is_some_and(|index| is_unquoted_word(&tokens[index], "as"))
            || previous.is_some_and(|index| tokens[index].token == Token::RParen)
        {
            replace_word(&mut tokens[top], "top", Keyword::NoKeyword);
        }
    }
}

fn normalize_non_reserved_projection_words(tokens: &mut [TokenWithSpan]) {
    for select in 0..tokens.len() {
        if !is_unquoted_word(&tokens[select], "select") {
            continue;
        }
        let Some(first) = next_significant(tokens, select + 1, tokens.len()) else {
            continue;
        };
        let Some(comma) = next_significant(tokens, first + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[first], "all") || tokens[comma].token != Token::Comma {
            continue;
        }
        let mut depth = 0usize;
        for token in tokens.iter_mut().skip(first) {
            match token.token {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => depth = depth.saturating_sub(1),
                _ if depth == 0 && is_unquoted_word(token, "from") => break,
                _ if depth == 0
                    && ["all", "some", "any"]
                        .iter()
                        .any(|word| is_unquoted_word(token, word)) =>
                {
                    let value = match &token.token {
                        Token::Word(word) => word.value.clone(),
                        _ => unreachable!(),
                    };
                    replace_word(token, &value, Keyword::NoKeyword);
                }
                _ => {}
            }
        }
    }
}

fn normalize_contextual_alias_words(tokens: &mut [TokenWithSpan]) {
    for index in 0..tokens.len() {
        let is_match = is_unquoted_word(&tokens[index], "match");
        let is_partition = is_unquoted_word(&tokens[index], "partition");
        if !is_match && !is_partition {
            continue;
        }
        let Some(previous) = previous_significant(tokens, index) else {
            continue;
        };
        let previous_can_end_value = matches!(
            &tokens[previous].token,
            Token::Number(_, _)
                | Token::SingleQuotedString(_)
                | Token::DoubleQuotedString(_)
                | Token::RParen
                | Token::RBracket
                | Token::RBrace
        ) || matches!(
            &tokens[previous].token,
            Token::Word(word)
                if word.quote_style.is_some()
                    || !matches!(
                        word.keyword,
                        Keyword::AFTER
                            | Keyword::AS
                            | Keyword::BY
                            | Keyword::FROM
                            | Keyword::MATCH
                            | Keyword::OVER
                            | Keyword::SELECT
                    )
        );
        if !previous_can_end_value {
            continue;
        }
        let next = next_significant(tokens, index + 1, tokens.len());
        if is_partition && next.is_some_and(|next| is_unquoted_word(&tokens[next], "by")) {
            continue;
        }
        let ends_alias = next.map_or(true, |next| {
            matches!(
                tokens[next].token,
                Token::Comma | Token::EOF | Token::LParen | Token::RParen | Token::SemiColon
            ) || matches!(
                unquoted_keyword(&tokens[next]),
                Some(
                    Keyword::EXCEPT
                        | Keyword::FETCH
                        | Keyword::FROM
                        | Keyword::GROUP
                        | Keyword::HAVING
                        | Keyword::INTERSECT
                        | Keyword::LIMIT
                        | Keyword::OFFSET
                        | Keyword::ORDER
                        | Keyword::UNION
                        | Keyword::WHERE
                        | Keyword::WINDOW
                )
            )
        });
        if ends_alias {
            let value = match &tokens[index].token {
                Token::Word(word) => word.value.clone(),
                _ => unreachable!(),
            };
            replace_word(&mut tokens[index], &value, Keyword::NoKeyword);
        }
    }
}

fn is_set_operator(token: &TokenWithSpan) -> bool {
    ["union", "intersect", "except"]
        .iter()
        .any(|operator| is_unquoted_word(token, operator))
}

fn corresponding_follows_set_operator(tokens: &[TokenWithSpan], corresponding: usize) -> bool {
    let Some(mut previous) = previous_significant(tokens, corresponding) else {
        return false;
    };
    if is_unquoted_word(&tokens[previous], "all") || is_unquoted_word(&tokens[previous], "distinct")
    {
        let Some(operator) = previous_significant(tokens, previous) else {
            return false;
        };
        previous = operator;
    }
    is_set_operator(&tokens[previous])
}

fn column_aliases_are_valid(tokens: &[TokenWithSpan], open: usize, close: usize) -> bool {
    let Some(mut alias) = next_significant(tokens, open + 1, close) else {
        return false;
    };
    loop {
        if !is_identifier(&tokens[alias]) {
            return false;
        }
        let Some(next) = next_significant(tokens, alias + 1, close) else {
            return true;
        };
        if tokens[next].token != Token::Comma {
            return false;
        }
        let Some(next_alias) = next_significant(tokens, next + 1, close) else {
            return false;
        };
        alias = next_alias;
    }
}

fn normalize_corresponding_set_operations(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for corresponding in 0..tokens.len() {
        if !is_unquoted_word(&tokens[corresponding], "corresponding")
            || !corresponding_follows_set_operator(tokens, corresponding)
        {
            continue;
        }
        let Some(next) = next_significant(tokens, corresponding + 1, tokens.len()) else {
            return Err(syntax_error(
                &tokens[corresponding],
                "CORRESPONDING requires a query",
            ));
        };
        let end = if is_unquoted_word(&tokens[next], "by") {
            let Some(open) = next_significant(tokens, next + 1, tokens.len()) else {
                return Err(syntax_error(
                    &tokens[next],
                    "CORRESPONDING BY requires column aliases",
                ));
            };
            if tokens[open].token != Token::LParen {
                return Err(syntax_error(
                    &tokens[open],
                    "CORRESPONDING BY requires parenthesized column aliases",
                ));
            }
            let Some(close) = matching_rparen(tokens, open) else {
                return Err(syntax_error(
                    &tokens[open],
                    "CORRESPONDING BY requires closed column aliases",
                ));
            };
            if !column_aliases_are_valid(tokens, open, close) {
                return Err(syntax_error(
                    &tokens[open],
                    "CORRESPONDING BY requires a comma-separated column-alias list",
                ));
            }
            close + 1
        } else {
            corresponding + 1
        };
        blank_non_whitespace(tokens, corresponding, end);
    }
    Ok(())
}

fn pivot_has_for_in_clause(tokens: &[TokenWithSpan], open: usize, before: usize) -> bool {
    let mut depth = 0usize;
    let mut for_index = None;
    for (index, token) in tokens.iter().enumerate().take(before).skip(open + 1) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
            }
            _ if depth == 0 && is_unquoted_word(token, "for") => for_index = Some(index),
            _ if depth == 0 && for_index.is_some() && is_unquoted_word(token, "in") => return true,
            _ => {}
        }
    }
    false
}

fn group_by_is_valid(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> bool {
    let group_by = tokens[start..end]
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .map(|token| token.token.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    !group_by.is_empty() && Parser::parse_sql(dialect, &format!("SELECT 1 {group_by}")).is_ok()
}

fn is_group_by_terminator(token: &TokenWithSpan) -> bool {
    matches!(token.token, Token::RParen | Token::SemiColon)
        || [
            "from",
            "where",
            "having",
            "window",
            "order",
            "limit",
            "offset",
            "fetch",
            "union",
            "intersect",
            "except",
        ]
        .iter()
        .any(|word| is_unquoted_word(token, word))
}

fn normalize_group_by_quantifiers(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for group in 0..tokens.len() {
        if !is_unquoted_word(&tokens[group], "group") {
            continue;
        }
        let Some(by) = next_significant(tokens, group + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[by], "by") {
            continue;
        }
        let end = statement_end(tokens, group);
        let Some(quantifier) = next_significant(tokens, by + 1, end) else {
            continue;
        };
        if !is_unquoted_word(&tokens[quantifier], "all")
            && !is_unquoted_word(&tokens[quantifier], "distinct")
        {
            continue;
        }
        let Some(element) = next_significant(tokens, quantifier + 1, end) else {
            return Err(syntax_error(
                &tokens[quantifier],
                "GROUP BY ALL or DISTINCT requires a grouping element",
            ));
        };
        if is_group_by_terminator(&tokens[element]) {
            return Err(syntax_error(
                &tokens[quantifier],
                "GROUP BY ALL or DISTINCT requires a grouping element",
            ));
        }
        blank_non_whitespace(tokens, quantifier, quantifier + 1);
    }
    Ok(())
}

fn normalize_group_by_auto(tokens: &mut [TokenWithSpan]) {
    for auto in 0..tokens.len() {
        if !is_unquoted_word(&tokens[auto], "auto") {
            continue;
        }
        let Some(by) = previous_significant(tokens, auto) else {
            continue;
        };
        let Some(group) = previous_significant(tokens, by) else {
            continue;
        };
        if is_unquoted_word(&tokens[by], "by") && is_unquoted_word(&tokens[group], "group") {
            replace_word(
                &mut tokens[auto],
                "__trino_group_by_auto",
                Keyword::NoKeyword,
            );
        }
    }
}

fn is_grouping_element_position(tokens: &[TokenWithSpan], before: usize) -> bool {
    let mut depth = 0usize;
    for index in (0..before).rev() {
        match tokens[index].token {
            Token::Whitespace(_) => continue,
            Token::RParen | Token::RBracket | Token::RBrace => depth += 1,
            Token::LParen | Token::LBracket | Token::LBrace => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
            }
            _ if depth > 0 => continue,
            _ if is_unquoted_word(&tokens[index], "by") => {
                return previous_significant(tokens, index)
                    .is_some_and(|group| is_unquoted_word(&tokens[group], "group"));
            }
            _ if ["select", "where", "having", "order", "limit", "offset"]
                .iter()
                .any(|word| is_unquoted_word(&tokens[index], word)) =>
            {
                return false
            }
            _ => {}
        }
    }
    false
}

fn validate_trino_grouping_elements(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for grouping in 0..tokens.len() {
        if !is_unquoted_word(&tokens[grouping], "rollup")
            && !is_unquoted_word(&tokens[grouping], "cube")
        {
            continue;
        }
        let Some(open) = next_significant(tokens, grouping + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        if !is_grouping_element_position(tokens, grouping) {
            return Err(syntax_error(
                &tokens[grouping],
                "ROLLUP and CUBE are grouping elements, not scalar functions",
            ));
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        if next_significant(tokens, open + 1, close).is_none() {
            return Err(syntax_error(
                &tokens[grouping],
                "ROLLUP and CUBE require at least one grouping set",
            ));
        }
    }
    Ok(())
}

fn normalize_pivot_group_by(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    for pivot in 0..tokens.len() {
        if !is_unquoted_word(&tokens[pivot], "pivot")
            || !has_relation_introducer_before(tokens, pivot)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, pivot + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let mut depth = 0usize;
        let mut group = None;
        for (index, token) in tokens.iter().enumerate().take(close).skip(open + 1) {
            match token.token {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => {
                    let Some(next_depth) = depth.checked_sub(1) else {
                        return Err(syntax_error(&tokens[index], "unbalanced PIVOT group"));
                    };
                    depth = next_depth;
                }
                _ if depth == 0 && is_unquoted_word(token, "group") => {
                    group = Some(index);
                    break;
                }
                _ => {}
            }
        }
        let Some(group) = group else {
            continue;
        };
        let Some(by) = next_significant(tokens, group + 1, close) else {
            return Err(syntax_error(&tokens[group], "PIVOT GROUP requires BY"));
        };
        if !is_unquoted_word(&tokens[by], "by") {
            return Err(syntax_error(&tokens[by], "PIVOT GROUP requires BY"));
        }
        if !pivot_has_for_in_clause(tokens, open, group) {
            return Err(syntax_error(
                &tokens[group],
                "PIVOT GROUP BY must follow the FOR ... IN clause",
            ));
        }
        if !group_by_is_valid(tokens, group, close, dialect) {
            return Err(syntax_error(
                &tokens[group],
                "invalid PIVOT GROUP BY clause",
            ));
        }
        let source = tokens[group].clone();
        let mut statement = Vec::with_capacity(close - group + 3);
        statement.push(word_with_span(&source, "SELECT", Keyword::SELECT));
        statement.push(token_with_span(
            Token::Number("1".to_string(), false),
            &source,
        ));
        statement.extend_from_slice(&tokens[group..close]);
        statement.push(token_with_span(Token::EOF, &tokens[close]));
        let mut parsed = Parser::new(dialect)
            .with_tokens_with_locations(statement)
            .parse_statements()?;
        metadata.append(&mut parsed);
        blank_non_whitespace(tokens, group, close);
    }
    Ok(metadata)
}

fn expression_tokens_are_valid(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> bool {
    let expression = tokens[start..end]
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .map(|token| token.token.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    !expression.is_empty() && Parser::parse_sql(dialect, &format!("SELECT {expression}")).is_ok()
}

fn parse_expression_metadata(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> Result<Statement, ParserError> {
    let Some(source) = next_significant(tokens, start, end) else {
        return Err(syntax_error(
            &tokens[start.saturating_sub(1)],
            "ROW requires a field expression",
        ));
    };
    let mut expression = Vec::with_capacity(end - start + 2);
    expression.push(word_with_span(&tokens[source], "SELECT", Keyword::SELECT));
    expression.extend_from_slice(&tokens[start..end]);
    let eof_source = tokens.get(end).unwrap_or(&tokens[source]);
    expression.push(token_with_span(Token::EOF, eof_source));
    let mut parsed = Parser::new(dialect)
        .with_tokens_with_locations(expression)
        .parse_statements()?;
    if parsed.len() != 1 {
        return Err(syntax_error(
            &tokens[source],
            "invalid ROW field expression",
        ));
    }
    parsed
        .pop()
        .ok_or_else(|| syntax_error(&tokens[source], "invalid ROW field expression"))
}

fn parse_order_by_metadata(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> Result<Statement, ParserError> {
    let Some(source) = next_significant(tokens, start, end) else {
        return Err(syntax_error(
            &tokens[start.saturating_sub(1)],
            "ORDER BY requires a sort item",
        ));
    };
    let mut statement = Vec::with_capacity(end - start + 5);
    statement.push(word_with_span(&tokens[source], "SELECT", Keyword::SELECT));
    statement.push(token_with_span(
        Token::Number("1".to_string(), false),
        &tokens[source],
    ));
    statement.push(word_with_span(&tokens[source], "ORDER", Keyword::ORDER));
    statement.push(word_with_span(&tokens[source], "BY", Keyword::BY));
    statement.extend_from_slice(&tokens[start..end]);
    statement.push(token_with_span(Token::EOF, &tokens[end]));
    let mut parsed = Parser::new(dialect)
        .with_tokens_with_locations(statement)
        .parse_statements()?;
    if parsed.len() != 1 {
        return Err(syntax_error(&tokens[source], "invalid ORDER BY clause"));
    }
    parsed
        .pop()
        .ok_or_else(|| syntax_error(&tokens[source], "invalid ORDER BY clause"))
}

fn table_function_call_open(tokens: &[TokenWithSpan], table: usize) -> Option<(usize, usize)> {
    let function_open = enclosing_parentheses(tokens, table).last().copied()?;
    let function_name = previous_significant(tokens, function_open)?;
    if !is_identifier(&tokens[function_name]) {
        return None;
    }
    let outer_open = enclosing_parentheses(tokens, function_open)
        .last()
        .copied()?;
    let outer_table = previous_significant(tokens, outer_open)?;
    if !is_unquoted_word(&tokens[outer_table], "table") {
        return None;
    }
    matching_rparen(tokens, function_open).map(|close| (function_open, close))
}

fn table_argument_boundary(
    tokens: &[TokenWithSpan],
    start: usize,
    function_close: usize,
    terminators: &[&str],
) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(function_close).skip(start) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace if depth > 0 => depth -= 1,
            Token::Comma if depth == 0 => return index,
            _ if depth == 0 && terminators.iter().any(|word| is_unquoted_word(token, word)) => {
                return index
            }
            _ => {}
        }
    }
    function_close
}

fn table_argument_option(token: &TokenWithSpan) -> bool {
    ["partition", "prune", "keep", "order", "copartition"]
        .iter()
        .any(|word| is_unquoted_word(token, word))
}

fn normalize_table_function_arguments(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    for table in 0..tokens.len() {
        if !is_unquoted_word(&tokens[table], "table") {
            continue;
        }
        let Some((_, function_close)) = table_function_call_open(tokens, table) else {
            continue;
        };
        let Some(open) = next_significant(tokens, table + 1, function_close) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(relation_close) = matching_rparen(tokens, open) else {
            continue;
        };
        if relation_close >= function_close {
            continue;
        }
        let Some(relation_start) = next_significant(tokens, open + 1, relation_close) else {
            return Err(syntax_error(
                &tokens[open],
                "table function table argument requires a relation",
            ));
        };
        if is_root_query_start(&tokens[relation_start]) {
            let mut query = tokens[open + 1..relation_close].to_vec();
            query.push(token_with_span(Token::EOF, &tokens[relation_close]));
            let mut parsed = Parser::new(dialect)
                .with_tokens_with_locations(query)
                .parse_statements()?;
            if parsed.len() != 1 {
                return Err(syntax_error(
                    &tokens[relation_start],
                    "table argument query must contain one query",
                ));
            }
            metadata.append(&mut parsed);
            replace_word(
                &mut tokens[relation_start],
                "__trino_table_argument",
                Keyword::NoKeyword,
            );
            blank_non_whitespace(tokens, relation_start + 1, relation_close);
        }
        let Some(mut cursor) = next_significant(tokens, relation_close + 1, function_close) else {
            continue;
        };
        if tokens[cursor].token == Token::Comma || is_unquoted_word(&tokens[cursor], "copartition")
        {
            continue;
        }
        let suffix_start = cursor;

        if is_unquoted_word(&tokens[cursor], "as") {
            cursor = next_significant(tokens, cursor + 1, function_close).ok_or_else(|| {
                syntax_error(&tokens[suffix_start], "table alias requires a name")
            })?;
            if !is_identifier(&tokens[cursor]) {
                return Err(syntax_error(
                    &tokens[cursor],
                    "invalid table argument alias",
                ));
            }
            cursor = next_significant(tokens, cursor + 1, function_close).unwrap_or(function_close);
            if cursor < function_close && tokens[cursor].token == Token::LParen {
                let close = matching_rparen(tokens, cursor).ok_or_else(|| {
                    syntax_error(
                        &tokens[cursor],
                        "unterminated table argument column aliases",
                    )
                })?;
                if !column_aliases_are_valid(tokens, cursor, close) {
                    return Err(syntax_error(
                        &tokens[cursor],
                        "table argument aliases require a nonempty column list",
                    ));
                }
                cursor =
                    next_significant(tokens, close + 1, function_close).unwrap_or(function_close);
            }
        } else if is_identifier(&tokens[cursor]) && !table_argument_option(&tokens[cursor]) {
            cursor = next_significant(tokens, cursor + 1, function_close).unwrap_or(function_close);
            if cursor < function_close && tokens[cursor].token == Token::LParen {
                let close = matching_rparen(tokens, cursor).ok_or_else(|| {
                    syntax_error(
                        &tokens[cursor],
                        "unterminated table argument column aliases",
                    )
                })?;
                if !column_aliases_are_valid(tokens, cursor, close) {
                    return Err(syntax_error(
                        &tokens[cursor],
                        "table argument aliases require a nonempty column list",
                    ));
                }
                cursor =
                    next_significant(tokens, close + 1, function_close).unwrap_or(function_close);
            }
        }

        if cursor < function_close && is_unquoted_word(&tokens[cursor], "partition") {
            let by = next_significant(tokens, cursor + 1, function_close)
                .ok_or_else(|| syntax_error(&tokens[cursor], "PARTITION requires BY"))?;
            if !is_unquoted_word(&tokens[by], "by") {
                return Err(syntax_error(&tokens[by], "PARTITION requires BY"));
            }
            let start = next_significant(tokens, by + 1, function_close)
                .ok_or_else(|| syntax_error(&tokens[by], "PARTITION BY requires an expression"))?;
            if tokens[start].token == Token::LParen {
                let close = matching_rparen(tokens, start).ok_or_else(|| {
                    syntax_error(&tokens[start], "unterminated PARTITION BY expression list")
                })?;
                if close >= function_close {
                    return Err(syntax_error(&tokens[start], "invalid PARTITION BY clause"));
                }
                if next_significant(tokens, start + 1, close).is_some() {
                    metadata.push(parse_expression_metadata(
                        tokens,
                        start + 1,
                        close,
                        dialect,
                    )?);
                }
                cursor =
                    next_significant(tokens, close + 1, function_close).unwrap_or(function_close);
            } else {
                let end = table_argument_boundary(
                    tokens,
                    start,
                    function_close,
                    &["prune", "keep", "order", "copartition"],
                );
                if next_significant(tokens, start, end).is_none() {
                    return Err(syntax_error(
                        &tokens[start],
                        "PARTITION BY requires an expression",
                    ));
                }
                metadata.push(parse_expression_metadata(tokens, start, end, dialect)?);
                cursor = next_significant(tokens, end, function_close).unwrap_or(function_close);
            }
        }

        if cursor < function_close
            && (is_unquoted_word(&tokens[cursor], "prune")
                || is_unquoted_word(&tokens[cursor], "keep"))
        {
            let treatment = cursor;
            let when = next_significant(tokens, cursor + 1, function_close).ok_or_else(|| {
                syntax_error(&tokens[treatment], "table treatment requires WHEN EMPTY")
            })?;
            let empty = next_significant(tokens, when + 1, function_close).ok_or_else(|| {
                syntax_error(&tokens[treatment], "table treatment requires WHEN EMPTY")
            })?;
            if !is_unquoted_word(&tokens[when], "when")
                || !is_unquoted_word(&tokens[empty], "empty")
            {
                return Err(syntax_error(
                    &tokens[treatment],
                    "table treatment requires WHEN EMPTY",
                ));
            }
            cursor = next_significant(tokens, empty + 1, function_close).unwrap_or(function_close);
        }

        if cursor < function_close && is_unquoted_word(&tokens[cursor], "order") {
            let by = next_significant(tokens, cursor + 1, function_close)
                .ok_or_else(|| syntax_error(&tokens[cursor], "ORDER requires BY"))?;
            if !is_unquoted_word(&tokens[by], "by") {
                return Err(syntax_error(&tokens[by], "ORDER requires BY"));
            }
            let start = next_significant(tokens, by + 1, function_close)
                .ok_or_else(|| syntax_error(&tokens[by], "ORDER BY requires a sort item"))?;
            if tokens[start].token == Token::LParen {
                let close = matching_rparen(tokens, start).ok_or_else(|| {
                    syntax_error(&tokens[start], "unterminated ORDER BY sort list")
                })?;
                if close >= function_close || next_significant(tokens, start + 1, close).is_none() {
                    return Err(syntax_error(
                        &tokens[start],
                        "ORDER BY requires a sort item",
                    ));
                }
                metadata.push(parse_order_by_metadata(tokens, start + 1, close, dialect)?);
                cursor =
                    next_significant(tokens, close + 1, function_close).unwrap_or(function_close);
            } else {
                let end = table_argument_boundary(tokens, start, function_close, &["copartition"]);
                metadata.push(parse_order_by_metadata(tokens, start, end, dialect)?);
                cursor = next_significant(tokens, end, function_close).unwrap_or(function_close);
            }
        }

        if cursor < function_close
            && tokens[cursor].token != Token::Comma
            && !is_unquoted_word(&tokens[cursor], "copartition")
        {
            return Err(syntax_error(
                &tokens[cursor],
                "unexpected token in table function table argument",
            ));
        }
        blank_non_whitespace(tokens, suffix_start, cursor);
    }
    Ok(metadata)
}

fn normalize_table_function_copartition(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for copartition in 0..tokens.len() {
        if !is_unquoted_word(&tokens[copartition], "copartition") {
            continue;
        }
        let Some((_, function_close)) = table_function_call_open(tokens, copartition) else {
            continue;
        };
        if previous_significant(tokens, copartition).is_some_and(|previous| {
            if tokens[previous].token != Token::RParen {
                return false;
            }
            matching_lparen(tokens, previous)
                .and_then(|open| previous_significant(tokens, open))
                .is_some_and(|word| is_unquoted_word(&tokens[word], "table"))
        }) {
            return Err(syntax_error(
                &tokens[copartition],
                "ambiguous COPARTITION after an unaliased table argument",
            ));
        }
        let mut cursor = next_significant(tokens, copartition + 1, function_close)
            .ok_or_else(|| syntax_error(&tokens[copartition], "COPARTITION requires tables"))?;
        loop {
            if tokens[cursor].token != Token::LParen {
                return Err(syntax_error(
                    &tokens[cursor],
                    "COPARTITION requires a parenthesized table list",
                ));
            }
            let close = matching_rparen(tokens, cursor).ok_or_else(|| {
                syntax_error(&tokens[cursor], "unterminated COPARTITION table list")
            })?;
            let first = next_significant(tokens, cursor + 1, close).ok_or_else(|| {
                syntax_error(&tokens[cursor], "COPARTITION requires at least two tables")
            })?;
            let first_end = consume_qualified_name(tokens, first, close)
                .ok_or_else(|| syntax_error(&tokens[first], "invalid COPARTITION table name"))?;
            let comma = next_significant(tokens, first_end, close).ok_or_else(|| {
                syntax_error(&tokens[first], "COPARTITION requires at least two tables")
            })?;
            if tokens[comma].token != Token::Comma {
                return Err(syntax_error(
                    &tokens[comma],
                    "COPARTITION tables must be comma-separated",
                ));
            }
            let mut table = next_significant(tokens, comma + 1, close).ok_or_else(|| {
                syntax_error(&tokens[comma], "COPARTITION requires a table after comma")
            })?;
            loop {
                let table_end = consume_qualified_name(tokens, table, close).ok_or_else(|| {
                    syntax_error(&tokens[table], "invalid COPARTITION table name")
                })?;
                let Some(separator) = next_significant(tokens, table_end, close) else {
                    break;
                };
                if tokens[separator].token != Token::Comma {
                    return Err(syntax_error(
                        &tokens[separator],
                        "COPARTITION tables must be comma-separated",
                    ));
                }
                table = next_significant(tokens, separator + 1, close).ok_or_else(|| {
                    syntax_error(
                        &tokens[separator],
                        "COPARTITION requires a table after comma",
                    )
                })?;
            }
            cursor = next_significant(tokens, close + 1, function_close).unwrap_or(function_close);
            if cursor == function_close {
                blank_non_whitespace(tokens, copartition, function_close);
                break;
            }
            if tokens[cursor].token != Token::Comma {
                return Err(syntax_error(
                    &tokens[cursor],
                    "unexpected token after COPARTITION table list",
                ));
            }
            cursor = next_significant(tokens, cursor + 1, function_close).ok_or_else(|| {
                syntax_error(&tokens[cursor], "COPARTITION requires another table list")
            })?;
        }
    }
    Ok(())
}

fn row_field_ranges(
    tokens: &[TokenWithSpan],
    open: usize,
    close: usize,
) -> Result<Vec<(usize, usize)>, ParserError> {
    let mut fields = Vec::new();
    let mut start = open + 1;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(close).skip(open + 1) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    syntax_error(token, "unbalanced group in ROW field expression")
                })?
            }
            Token::Comma if depth == 0 => {
                if next_significant(tokens, start, index).is_none() {
                    return Err(syntax_error(token, "ROW requires a field expression"));
                }
                fields.push((start, index));
                start = index + 1;
            }
            _ => {}
        }
    }
    if next_significant(tokens, start, close).is_none() {
        return Err(syntax_error(
            &tokens[open],
            "ROW requires a field expression",
        ));
    }
    fields.push((start, close));
    Ok(fields)
}

fn has_top_level_row_clause(tokens: &[TokenWithSpan], start: usize, end: usize) -> bool {
    let mut depth = 0usize;
    for token in &tokens[start..end] {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return true;
                };
                depth = next_depth;
            }
            _ if depth == 0
                && [
                    "from",
                    "where",
                    "group",
                    "having",
                    "order",
                    "limit",
                    "offset",
                    "union",
                    "intersect",
                    "except",
                    "fetch",
                ]
                .iter()
                .any(|word| is_unquoted_word(token, word)) =>
            {
                return true
            }
            _ => {}
        }
    }
    false
}

fn row_column_aliases_end(
    tokens: &[TokenWithSpan],
    asterisk: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> Result<usize, ParserError> {
    let Some(as_index) = next_significant(tokens, asterisk + 1, end) else {
        return Ok(asterisk + 1);
    };
    if !is_unquoted_word(&tokens[as_index], "as") {
        return Ok(asterisk + 1);
    }
    let Some(open) = next_significant(tokens, as_index + 1, end) else {
        return Err(syntax_error(
            &tokens[as_index],
            "ROW .* AS requires column aliases",
        ));
    };
    if tokens[open].token != Token::LParen {
        return Err(syntax_error(
            &tokens[open],
            "ROW .* AS requires parenthesized column aliases",
        ));
    }
    let Some(close) = matching_rparen(tokens, open) else {
        return Err(syntax_error(
            &tokens[open],
            "unterminated ROW .* column aliases",
        ));
    };
    let mut expect_identifier = true;
    for token in tokens.iter().take(close).skip(open + 1) {
        if matches!(token.token, Token::Whitespace(_)) {
            continue;
        }
        if expect_identifier {
            if !is_branch_identifier(token, dialect) {
                return Err(syntax_error(
                    token,
                    "ROW .* column aliases require identifiers",
                ));
            }
        } else if token.token != Token::Comma {
            return Err(syntax_error(token, "ROW .* column aliases require commas"));
        }
        expect_identifier = !expect_identifier;
    }
    if expect_identifier {
        return Err(syntax_error(
            &tokens[open],
            "ROW .* AS requires one or more column aliases",
        ));
    }
    Ok(close + 1)
}

fn normalize_row_expansions(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    for row in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[row], "row") {
            continue;
        }
        let Some(open) = next_significant(tokens, row + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let Some(period) = next_significant(tokens, close + 1, tokens.len()) else {
            continue;
        };
        let Some(asterisk) = next_significant(tokens, period + 1, tokens.len()) else {
            continue;
        };
        if tokens[period].token != Token::Period || tokens[asterisk].token != Token::Mul {
            continue;
        }
        let aliases_end = row_column_aliases_end(tokens, asterisk, tokens.len(), dialect)?;
        for (start, end) in row_field_ranges(tokens, open, close)? {
            if has_top_level_row_clause(tokens, start, end) {
                return Err(syntax_error(&tokens[start], "invalid ROW field expression"));
            }
            metadata.push(parse_expression_metadata(tokens, start, end, dialect)?);
        }
        replace_word(
            &mut tokens[row],
            "__trino_row_expansion",
            Keyword::NoKeyword,
        );
        blank_non_whitespace(tokens, open, close + 1);
        blank_non_whitespace(tokens, asterisk + 1, aliases_end);
    }
    Ok(metadata)
}

fn nearest_clause_indices(
    tokens: &[TokenWithSpan],
    open: usize,
    close: usize,
) -> Result<(usize, Option<usize>, usize), ParserError> {
    let Some(from) = next_significant(tokens, open + 1, close) else {
        return Err(syntax_error(&tokens[open], "NEAREST requires FROM"));
    };
    if !is_unquoted_word(&tokens[from], "from") {
        return Err(syntax_error(&tokens[from], "NEAREST requires FROM"));
    }
    let mut depth = 0usize;
    let mut where_index = None;
    let mut match_index = None;
    for (index, token) in tokens.iter().enumerate().take(close).skip(from + 1) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return Err(syntax_error(token, "unbalanced NEAREST group"));
                };
                depth = next_depth;
            }
            _ if depth == 0 && is_unquoted_word(token, "where") => {
                if where_index.is_some() || match_index.is_some() {
                    return Err(syntax_error(token, "invalid NEAREST WHERE clause"));
                }
                where_index = Some(index);
            }
            _ if depth == 0 && is_unquoted_word(token, "match") => {
                if match_index.is_some() {
                    return Err(syntax_error(token, "invalid NEAREST MATCH clause"));
                }
                match_index = Some(index);
            }
            _ => {}
        }
    }
    let Some(match_index) = match_index else {
        return Err(syntax_error(&tokens[open], "NEAREST requires MATCH"));
    };
    let relation_end = where_index.unwrap_or(match_index);
    if next_significant(tokens, from + 1, relation_end).is_none() {
        return Err(syntax_error(&tokens[from], "NEAREST requires a relation"));
    }
    if let Some(where_index) = where_index {
        let Some(condition) = next_significant(tokens, where_index + 1, match_index) else {
            return Err(syntax_error(
                &tokens[where_index],
                "NEAREST WHERE requires a condition",
            ));
        };
        if condition >= match_index {
            return Err(syntax_error(
                &tokens[where_index],
                "NEAREST WHERE requires a condition",
            ));
        }
    }
    if next_significant(tokens, match_index + 1, close).is_none() {
        return Err(syntax_error(
            &tokens[match_index],
            "NEAREST MATCH requires a condition",
        ));
    }
    Ok((from, where_index, match_index))
}

fn normalize_nearest_relations(
    tokens: &mut Vec<TokenWithSpan>,
    dialect: &dyn Dialect,
) -> Result<(), ParserError> {
    for nearest in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[nearest], "nearest")
            || !has_relation_introducer_before(tokens, nearest)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, nearest + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let (from, where_index, match_index) = nearest_clause_indices(tokens, open, close)?;
        if !expression_tokens_are_valid(tokens, match_index + 1, close, dialect) {
            return Err(syntax_error(
                &tokens[match_index],
                "invalid NEAREST MATCH condition",
            ));
        }
        if let Some(where_index) = where_index {
            if !expression_tokens_are_valid(tokens, where_index + 1, match_index, dialect) {
                return Err(syntax_error(
                    &tokens[where_index],
                    "invalid NEAREST WHERE condition",
                ));
            }
        }
        let source = tokens[from].clone();
        tokens[nearest].token = Token::Whitespace(Whitespace::Space);
        tokens.insert(from, word_with_span(&source, "SELECT", Keyword::SELECT));
        tokens.insert(from + 1, token_with_span(Token::Mul, &source));
        let match_index = match_index + 2;
        if where_index.is_some() {
            replace_word(&mut tokens[match_index], "AND", Keyword::AND);
        } else {
            replace_word(&mut tokens[match_index], "WHERE", Keyword::WHERE);
        }
    }
    Ok(())
}

fn normalize_ipaddress_literals(tokens: &mut Vec<TokenWithSpan>) {
    for ipaddress in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[ipaddress], "ipaddress") {
            continue;
        }
        let Some(value) = next_significant(tokens, ipaddress + 1, tokens.len()) else {
            continue;
        };
        if !is_single_quoted_string(&tokens[value]) {
            continue;
        }
        let source = tokens[ipaddress].clone();
        replace_word(&mut tokens[ipaddress], "CAST", Keyword::CAST);
        tokens.insert(ipaddress + 1, token_with_span(Token::LParen, &source));
        let value = value + 1;
        tokens.insert(value + 1, word_with_span(&source, "AS", Keyword::AS));
        tokens.insert(
            value + 2,
            word_with_span(&source, "IPADDRESS", Keyword::NoKeyword),
        );
        tokens.insert(value + 3, token_with_span(Token::RParen, &source));
    }
}

fn can_end_expression_before_at(tokens: &[TokenWithSpan], at: usize) -> bool {
    let Some(previous) = previous_significant(tokens, at) else {
        return false;
    };
    match &tokens[previous].token {
        Token::Word(word) if word.quote_style.is_some() => true,
        Token::Word(word) => ![
            "all",
            "and",
            "as",
            "between",
            "by",
            "distinct",
            "else",
            "from",
            "group",
            "having",
            "in",
            "join",
            "like",
            "not",
            "on",
            "or",
            "order",
            "over",
            "partition",
            "return",
            "select",
            "then",
            "values",
            "when",
            "where",
        ]
        .iter()
        .any(|keyword| word.value.eq_ignore_ascii_case(keyword)),
        Token::Number(_, _)
        | Token::SingleQuotedString(_)
        | Token::UnicodeStringLiteral(_)
        | Token::HexStringLiteral(_)
        | Token::Placeholder(_)
        | Token::RParen
        | Token::RBracket
        | Token::RBrace => true,
        _ => false,
    }
}

fn normalize_at_syntax(tokens: &mut Vec<TokenWithSpan>) {
    for at in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[at], "at") {
            continue;
        }
        let Some(modifier) = next_significant(tokens, at + 1, tokens.len()) else {
            if let Token::Word(word) = &mut tokens[at].token {
                word.keyword = Keyword::NoKeyword;
            }
            continue;
        };
        let can_be_operator = can_end_expression_before_at(tokens, at);
        if can_be_operator && is_unquoted_word(&tokens[modifier], "local") {
            let source = tokens[modifier].clone();
            replace_word(&mut tokens[modifier], "TIME", Keyword::TIME);
            tokens.insert(modifier + 1, word_with_span(&source, "ZONE", Keyword::ZONE));
            tokens.insert(
                modifier + 2,
                token_with_span(Token::SingleQuotedString("UTC".to_string()), &source),
            );
        } else if !(can_be_operator && is_unquoted_word(&tokens[modifier], "time")) {
            if let Token::Word(word) = &mut tokens[at].token {
                word.keyword = Keyword::NoKeyword;
            }
        }
    }
}

fn is_relation_time_travel_position(tokens: &[TokenWithSpan], for_index: usize) -> bool {
    let mut depth = 0usize;
    for index in (0..for_index).rev() {
        match tokens[index].token {
            Token::Whitespace(_) => continue,
            Token::RParen | Token::RBracket | Token::RBrace => depth += 1,
            Token::LParen | Token::LBracket | Token::LBrace => {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next_depth;
            }
            _ if depth > 0 => continue,
            Token::Comma => {
                return next_significant(tokens, index + 1, for_index)
                    .is_some_and(|start| is_qualified_name(tokens, start, for_index));
            }
            _ if is_unquoted_word(&tokens[index], "from")
                || is_unquoted_word(&tokens[index], "join") =>
            {
                return next_significant(tokens, index + 1, for_index)
                    .is_some_and(|start| is_qualified_name(tokens, start, for_index));
            }
            _ if [
                "where",
                "on",
                "group",
                "order",
                "having",
                "limit",
                "offset",
                "union",
                "intersect",
                "except",
                "select",
                "values",
            ]
            .iter()
            .any(|word| is_unquoted_word(&tokens[index], word)) =>
            {
                return false
            }
            _ => {}
        }
    }
    false
}

fn normalize_iceberg_time_travel(tokens: &mut [TokenWithSpan]) {
    for for_index in 0..tokens.len() {
        if !is_unquoted_word(&tokens[for_index], "for")
            || !is_relation_time_travel_position(tokens, for_index)
        {
            continue;
        }
        let Some(kind) = next_significant(tokens, for_index + 1, tokens.len()) else {
            continue;
        };
        let Some(as_index) = next_significant(tokens, kind + 1, tokens.len()) else {
            continue;
        };
        let Some(of) = next_significant(tokens, as_index + 1, tokens.len()) else {
            continue;
        };
        let Some(_value) = next_significant(tokens, of + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[as_index], "as") || !is_unquoted_word(&tokens[of], "of") {
            continue;
        }
        if is_unquoted_word(&tokens[kind], "timestamp") {
            tokens[for_index].token = Token::Whitespace(Whitespace::Space);
        } else if is_unquoted_word(&tokens[kind], "version") {
            tokens[for_index].token = Token::Whitespace(Whitespace::Space);
            replace_word(&mut tokens[kind], "TIMESTAMP", Keyword::TIMESTAMP);
        }
    }
}

fn scalar_values(tokens: &[TokenWithSpan], values: usize, end: usize) -> Option<Vec<usize>> {
    let mut value = next_significant(tokens, values + 1, end)?;
    let mut strings = Vec::new();
    loop {
        if !matches!(tokens[value].token, Token::SingleQuotedString(_)) {
            return None;
        }
        strings.push(value);
        let next = next_significant(tokens, value + 1, end)?;
        if tokens[next].token == Token::Comma {
            value = next_significant(tokens, next + 1, end)?;
            continue;
        }
        return (strings.len() > 1 && matches!(tokens[next].token, Token::RParen))
            .then_some(strings);
    }
}

fn normalize_scalar_values(tokens: &mut Vec<TokenWithSpan>) {
    let mut values = tokens.len();
    while let Some(index) = previous_significant(tokens, values) {
        values = index;
        if !is_unquoted_word(&tokens[index], "values") {
            continue;
        }
        let end = statement_end(tokens, index);
        let Some(strings) = scalar_values(tokens, index, end) else {
            continue;
        };
        for string in strings.into_iter().rev() {
            let source = tokens[string].clone();
            tokens.insert(string, token_with_span(Token::LParen, &source));
            tokens.insert(string + 2, token_with_span(Token::RParen, &source));
        }
    }
}

fn normalize_typed_values(tokens: &mut Vec<TokenWithSpan>) {
    for values in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[values], "values") {
            continue;
        }
        let Some(data_type) = next_significant(tokens, values + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[data_type], "varchar") {
            continue;
        }
        let Some(value) = next_significant(tokens, data_type + 1, tokens.len()) else {
            continue;
        };
        if !is_single_quoted_string(&tokens[value]) {
            continue;
        }
        let source = tokens[data_type].clone();
        replace_word(&mut tokens[data_type], "CAST", Keyword::CAST);
        tokens.insert(data_type, token_with_span(Token::LParen, &source));
        tokens.insert(data_type + 2, token_with_span(Token::LParen, &source));
        let value = value + 2;
        tokens.insert(value + 1, word_with_span(&source, "AS", Keyword::AS));
        tokens.insert(
            value + 2,
            word_with_span(&source, "VARCHAR", Keyword::VARCHAR),
        );
        tokens.insert(value + 3, token_with_span(Token::RParen, &source));
        tokens.insert(value + 4, token_with_span(Token::RParen, &source));
    }
}

fn normalize_values_expression(tokens: &mut Vec<TokenWithSpan>, function_name: Option<&str>) {
    for values in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[values], "values") {
            continue;
        }
        let Some(expression) = next_significant(tokens, values + 1, tokens.len()) else {
            continue;
        };
        let is_match = match function_name {
            Some(name) => is_unquoted_word(&tokens[expression], name),
            None => is_unquoted_word(&tokens[expression], "array"),
        };
        if !is_match {
            continue;
        }
        let Some(open) = next_significant(tokens, expression + 1, tokens.len()) else {
            continue;
        };
        let expected_open = if function_name.is_some() {
            Token::LParen
        } else {
            Token::LBracket
        };
        if tokens[open].token != expected_open {
            continue;
        }
        let close = if function_name.is_some() {
            matching_rparen(tokens, open)
        } else {
            let mut depth = 0usize;
            (open..tokens.len()).find(|index| match tokens[*index].token {
                Token::LBracket => {
                    depth += 1;
                    false
                }
                Token::RBracket => {
                    depth = depth.saturating_sub(1);
                    depth == 0
                }
                _ => false,
            })
        };
        let Some(close) = close else {
            continue;
        };
        let source = tokens[expression].clone();
        tokens.insert(expression, token_with_span(Token::LParen, &source));
        tokens.insert(close + 2, token_with_span(Token::RParen, &source));
    }
}

fn is_values_relation_position(tokens: &[TokenWithSpan], values: usize) -> bool {
    if has_relation_introducer_before(tokens, values) {
        return true;
    }
    let statement_start = statement_start(tokens, values);
    if next_significant(tokens, statement_start, values) == Some(values) {
        return true;
    }
    let first = next_significant(tokens, statement_start, values);
    let second = first.and_then(|index| next_significant(tokens, index + 1, values));
    if first.is_some_and(|index| is_unquoted_word(&tokens[index], "insert"))
        && second.is_some_and(|index| is_unquoted_word(&tokens[index], "into"))
        && enclosing_parentheses(tokens, values).is_empty()
    {
        return true;
    }
    let Some(open) = enclosing_parentheses(tokens, values).last().copied() else {
        return false;
    };
    previous_significant(tokens, open).is_some_and(|previous| {
        ["from", "join", "lateral", "as", "all", "any", "some"]
            .iter()
            .any(|word| is_unquoted_word(&tokens[previous], word))
    })
}

fn values_relation_expression_ranges(
    tokens: &[TokenWithSpan],
    values: usize,
    end: usize,
) -> Result<Option<Vec<(usize, usize)>>, ParserError> {
    if !is_values_relation_position(tokens, values) {
        return Ok(None);
    }
    let Some(first) = next_significant(tokens, values + 1, end) else {
        return Ok(None);
    };
    if tokens[first].token == Token::LParen {
        return Ok(None);
    }
    let mut expressions = Vec::new();
    let mut start = first;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(first) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace if depth > 0 => depth -= 1,
            Token::RParen if depth == 0 => {
                expressions.push((start, index));
                return Ok(Some(expressions));
            }
            Token::Comma if depth == 0 => {
                if next_significant(tokens, start, index).is_none() {
                    return Err(syntax_error(token, "VALUES requires an expression"));
                }
                expressions.push((start, index));
                start = next_significant(tokens, index + 1, end)
                    .ok_or_else(|| syntax_error(token, "VALUES requires an expression"))?;
            }
            _ if depth == 0
                && [
                    "order",
                    "limit",
                    "offset",
                    "fetch",
                    "union",
                    "intersect",
                    "except",
                ]
                .iter()
                .any(|word| is_unquoted_word(token, word)) =>
            {
                expressions.push((start, index));
                return Ok(Some(expressions));
            }
            _ => {}
        }
    }
    expressions.push((start, end));
    Ok(Some(expressions))
}

fn normalize_quantified_values(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    for values in 0..tokens.len() {
        if !is_unquoted_word(&tokens[values], "values") {
            continue;
        }
        let Some(open) = enclosing_parentheses(tokens, values).last().copied() else {
            continue;
        };
        let Some(quantifier) = previous_significant(tokens, open) else {
            continue;
        };
        if !["all", "any", "some"]
            .iter()
            .any(|word| is_unquoted_word(&tokens[quantifier], word))
        {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let Some(expressions) = values_relation_expression_ranges(tokens, values, close)? else {
            continue;
        };
        if expressions.is_empty() {
            return Err(syntax_error(
                &tokens[values],
                "quantified VALUES requires an expression",
            ));
        }
        for (start, end) in expressions {
            metadata.push(parse_expression_metadata(tokens, start, end, dialect)?);
        }
        replace_word(&mut tokens[values], "SELECT", Keyword::SELECT);
        let expression = next_significant(tokens, values + 1, close).ok_or_else(|| {
            syntax_error(&tokens[values], "quantified VALUES requires an expression")
        })?;
        tokens[expression] =
            token_with_span(Token::Number("1".to_string(), false), &tokens[expression]);
        blank_non_whitespace(tokens, expression + 1, close);
    }
    Ok(metadata)
}

fn normalize_scalar_values_relations(
    tokens: &mut Vec<TokenWithSpan>,
    dialect: &dyn Dialect,
) -> Result<(), ParserError> {
    for values in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[values], "values") {
            continue;
        }
        let end = statement_end(tokens, values);
        let Some(expressions) = values_relation_expression_ranges(tokens, values, end)? else {
            continue;
        };
        for (start, end) in expressions.into_iter().rev() {
            if !expression_tokens_are_valid(tokens, start, end, dialect) {
                return Err(syntax_error(&tokens[start], "invalid VALUES expression"));
            }
            let source = tokens[start].clone();
            tokens.insert(start, token_with_span(Token::LParen, &source));
            tokens.insert(end + 1, token_with_span(Token::RParen, &source));
        }
    }
    Ok(())
}

fn normalize_materialized_view_options(tokens: &mut [TokenWithSpan]) {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(view) = create_materialized_view_keyword(tokens, start, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let Some((as_index, clauses)) = materialized_view_options(tokens, view, end) else {
            start = end.saturating_add(1);
            continue;
        };
        let mut ranges = Vec::new();
        let mut valid = true;
        for (position, (clause, rank)) in clauses.iter().enumerate() {
            let boundary = clauses
                .get(position + 1)
                .map_or(as_index, |(next, _)| *next);
            match rank {
                0 => {
                    let Some(period) = next_significant(tokens, clause + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    let Some(interval) = next_significant(tokens, period + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    let Some(value) = next_significant(tokens, interval + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    let Some(unit) = next_significant(tokens, value + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    if !is_unquoted_word(&tokens[period], "period")
                        || !is_unquoted_word(&tokens[interval], "interval")
                        || !is_single_quoted_string(&tokens[value])
                        || !is_interval_unit(&tokens[unit])
                        || next_significant(tokens, unit + 1, boundary).is_some()
                    {
                        valid = false;
                        break;
                    }
                    ranges.push((*clause, boundary));
                }
                1 => {
                    let Some(stale) = next_significant(tokens, clause + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    let Some(behavior) = next_significant(tokens, stale + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    if !is_unquoted_word(&tokens[stale], "stale")
                        || !(is_unquoted_word(&tokens[behavior], "inline")
                            || is_unquoted_word(&tokens[behavior], "fail"))
                        || next_significant(tokens, behavior + 1, boundary).is_some()
                    {
                        valid = false;
                        break;
                    }
                    ranges.push((*clause, boundary));
                }
                2 => {
                    let Some(value) = next_significant(tokens, clause + 1, boundary) else {
                        valid = false;
                        break;
                    };
                    if !is_single_quoted_string(&tokens[value])
                        || next_significant(tokens, value + 1, boundary).is_some()
                    {
                        valid = false;
                        break;
                    }
                    ranges.push((*clause, boundary));
                }
                3 => {}
                _ => unreachable!(),
            }
        }
        if valid {
            for (range_start, range_end) in ranges {
                blank_non_whitespace(tokens, range_start, range_end);
            }
        }
        start = end.saturating_add(1);
    }
}

fn is_identifier(token: &TokenWithSpan) -> bool {
    matches!(token.token, Token::Word(_))
}

fn match_recognize_group(tokens: &[TokenWithSpan], index: usize) -> Option<(usize, usize)> {
    let open = *enclosing_parentheses(tokens, index).last()?;
    let keyword = previous_significant(tokens, open)?;
    if !is_unquoted_word(&tokens[keyword], "match_recognize") {
        return None;
    }
    matching_rparen(tokens, open).map(|close| (open, close))
}

fn match_recognize_subset_end(
    tokens: &[TokenWithSpan],
    subset: usize,
    close: usize,
) -> Option<usize> {
    let mut cursor = next_significant(tokens, subset + 1, close)?;
    loop {
        if !is_identifier(&tokens[cursor]) || is_unquoted_word(&tokens[cursor], "define") {
            return None;
        }
        cursor = next_significant(tokens, cursor + 1, close)?;
        if tokens[cursor].token != Token::Eq {
            return None;
        }
        cursor = next_significant(tokens, cursor + 1, close)?;
        if tokens[cursor].token != Token::LParen {
            return None;
        }
        let members_end = matching_rparen(tokens, cursor)?;
        if members_end >= close {
            return None;
        }
        let mut member = next_significant(tokens, cursor + 1, members_end)?;
        loop {
            if !is_identifier(&tokens[member]) {
                return None;
            }
            let next = next_significant(tokens, member + 1, members_end);
            let Some(comma) = next else {
                break;
            };
            if tokens[comma].token != Token::Comma {
                return None;
            }
            member = next_significant(tokens, comma + 1, members_end)?;
        }
        cursor = next_significant(tokens, members_end + 1, close)?;
        if is_unquoted_word(&tokens[cursor], "define") {
            return Some(cursor);
        }
        if tokens[cursor].token != Token::Comma {
            return None;
        }
        cursor = next_significant(tokens, cursor + 1, close)?;
    }
}

fn normalize_match_recognize_subsets(tokens: &mut [TokenWithSpan]) {
    for subset in 0..tokens.len() {
        if !is_unquoted_word(&tokens[subset], "subset") {
            continue;
        }
        let Some((_, close)) = match_recognize_group(tokens, subset) else {
            continue;
        };
        let Some(define) = match_recognize_subset_end(tokens, subset, close) else {
            continue;
        };
        blank_non_whitespace(tokens, subset, define);
    }
}

fn is_root_query_start(token: &TokenWithSpan) -> bool {
    matches!(token.token, Token::LParen)
        || ["select", "table", "values", "with"]
            .iter()
            .any(|word| is_unquoted_word(token, word))
}

fn is_inline_function_query_start(tokens: &[TokenWithSpan], index: usize, end: usize) -> bool {
    if tokens[index].token != Token::LParen {
        return is_root_query_start(&tokens[index]);
    }
    next_significant(tokens, index + 1, end).is_some_and(|inner| {
        !matches!(tokens[inner].token, Token::LParen) && is_root_query_start(&tokens[inner])
    })
}

fn find_top_level_query_prefix(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    prefix: &str,
) -> Option<usize> {
    let mut depth = 0usize;
    for index in start..end {
        match tokens[index].token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.saturating_sub(1);
            }
            _ if depth == 0 && is_unquoted_word(&tokens[index], "with") => {
                let next = next_significant(tokens, index + 1, end)?;
                if is_unquoted_word(&tokens[next], prefix) {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_inline_function_declaration(
    tokens: &[TokenWithSpan],
    function: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> Result<(usize, Statement), ParserError> {
    let mut depth = 0usize;
    let mut last_error = None;
    for (index, token) in tokens.iter().enumerate().take(end).skip(function + 1) {
        if matches!(token.token, Token::Whitespace(_)) {
            continue;
        }
        if depth == 0
            && (token.token == Token::Comma || is_inline_function_query_start(tokens, index, end))
        {
            match parse_inline_function(tokens, function, index, dialect) {
                Ok(statement) => return Ok((index, statement)),
                Err(error) => last_error = Some(error),
            }
        }
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    syntax_error(token, "unbalanced group in WITH FUNCTION expression")
                })?
            }
            _ => {}
        }
    }
    Err(last_error.unwrap_or_else(|| {
        syntax_error(
            &tokens[function],
            "WITH FUNCTION requires a complete declaration and following query",
        )
    }))
}

fn parse_inline_function(
    tokens: &[TokenWithSpan],
    function: usize,
    boundary: usize,
    dialect: &dyn Dialect,
) -> Result<Statement, ParserError> {
    let mut declaration = Vec::with_capacity(boundary - function + 2);
    declaration.push(word_with_span(&tokens[function], "CREATE", Keyword::CREATE));
    declaration.extend_from_slice(&tokens[function..boundary]);
    declaration.push(token_with_span(Token::EOF, &tokens[boundary]));
    let mut parsed = Parser::new(dialect)
        .with_tokens_with_locations(declaration)
        .parse_statements()?;
    if parsed.len() != 1 {
        return Err(syntax_error(
            &tokens[function],
            "invalid WITH FUNCTION declaration",
        ));
    }
    let statement = parsed.pop().expect("one checked inline function statement");
    if !matches!(statement, Statement::CreateFunction(_)) {
        return Err(syntax_error(
            &tokens[function],
            "invalid WITH FUNCTION declaration",
        ));
    }
    Ok(statement)
}

fn normalize_inline_functions(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<(usize, Statement)>, ParserError> {
    let mut declarations = Vec::new();
    let mut start = 0usize;
    let mut statement_index = 0usize;
    while start < tokens.len() {
        let ordinary_end = statement_end(tokens, start);
        let Some(with) = find_top_level_query_prefix(tokens, start, ordinary_end, "function")
        else {
            start = ordinary_end.saturating_add(1);
            statement_index += 1;
            continue;
        };
        let Some(mut function) = next_significant(tokens, with + 1, tokens.len()) else {
            break;
        };
        let end = tokens.len();

        let query_start = loop {
            let (boundary, declaration) =
                parse_inline_function_declaration(tokens, function, end, dialect)?;
            declarations.push((statement_index, declaration));
            if tokens[boundary].token == Token::Comma {
                let Some(next_function) = next_significant(tokens, boundary + 1, end) else {
                    return Err(syntax_error(
                        &tokens[boundary],
                        "WITH FUNCTION requires another declaration after ','",
                    ));
                };
                if !is_unquoted_word(&tokens[next_function], "function") {
                    return Err(syntax_error(
                        &tokens[next_function],
                        "WITH FUNCTION declarations must follow ','",
                    ));
                }
                function = next_function;
                continue;
            }
            blank_non_whitespace(tokens, with, boundary);
            break boundary;
        };
        start = statement_end(tokens, query_start).saturating_add(1);
        statement_index += 1;
    }
    Ok(declarations)
}

fn consume_qualified_name(tokens: &[TokenWithSpan], start: usize, end: usize) -> Option<usize> {
    let mut cursor = start;
    if !is_identifier(&tokens[cursor]) {
        return None;
    }
    loop {
        let period = next_significant(tokens, cursor + 1, end);
        let Some(period) = period else {
            return Some(cursor + 1);
        };
        if tokens[period].token != Token::Period {
            return Some(cursor + 1);
        }
        cursor = next_significant(tokens, period + 1, end)?;
        if !is_identifier(&tokens[cursor]) {
            return None;
        }
    }
}

fn session_property_value_end(tokens: &[TokenWithSpan], start: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut value = false;
    for (index, token_with_span) in tokens.iter().enumerate().take(end).skip(start) {
        let token = &token_with_span.token;
        if matches!(token, Token::Whitespace(_)) {
            continue;
        }
        if depth == 0 && value {
            if matches!(token, Token::Comma) {
                return Some(index);
            }
            if is_root_query_start(&tokens[index]) {
                let parenthesized_query = matches!(token, Token::LParen)
                    && next_significant(tokens, index + 1, end)
                        .is_some_and(|next| is_root_query_start(&tokens[next]));
                if !matches!(token, Token::LParen) || parenthesized_query {
                    return Some(index);
                }
            }
        }
        match token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => depth = depth.checked_sub(1)?,
            _ => {}
        }
        value = true;
    }
    None
}

fn session_property_value_is_valid(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> bool {
    let value = tokens[start..end]
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .map(|token| token.token.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    !value.is_empty() && Parser::parse_sql(dialect, &format!("SELECT {value}")).is_ok()
}

fn with_session_query_start(
    tokens: &[TokenWithSpan],
    start: usize,
    end: usize,
    dialect: &dyn Dialect,
) -> Option<(usize, Vec<(usize, usize)>)> {
    let with = next_significant(tokens, start, end)?;
    if !is_unquoted_word(&tokens[with], "with") {
        return None;
    }
    let session = next_significant(tokens, with + 1, end)?;
    if !is_unquoted_word(&tokens[session], "session") {
        return None;
    }
    let mut property = next_significant(tokens, session + 1, end)?;
    let mut values = Vec::new();
    loop {
        let after_name = consume_qualified_name(tokens, property, end)?;
        let equals = next_significant(tokens, after_name, end)?;
        if tokens[equals].token != Token::Eq {
            return None;
        }
        let value_start = next_significant(tokens, equals + 1, end)?;
        let value_end = session_property_value_end(tokens, value_start, end)?;
        if !session_property_value_is_valid(tokens, value_start, value_end, dialect) {
            return None;
        }
        values.push((value_start, value_end));
        if tokens[value_end].token == Token::Comma {
            property = next_significant(tokens, value_end + 1, end)?;
            continue;
        }
        return is_root_query_start(&tokens[value_end]).then_some((value_end, values));
    }
}

fn normalize_with_session(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let prefix = find_top_level_query_prefix(tokens, start, end, "session");
        if let Some((query_start, values)) =
            prefix.and_then(|with| with_session_query_start(tokens, with, end, dialect))
        {
            for (value_start, value_end) in values {
                metadata.push(parse_expression_metadata(
                    tokens,
                    value_start,
                    value_end,
                    dialect,
                )?);
            }
            let with = prefix.expect("matched WITH SESSION prefix");
            blank_non_whitespace(tokens, with, query_start);
        }
        start = end.saturating_add(1);
    }
    Ok(metadata)
}

fn dml_target_start(tokens: &[TokenWithSpan], at: usize) -> Option<usize> {
    let statement = statement_start(tokens, at);
    let first = next_significant(tokens, statement, at)?;
    let after_first = next_significant(tokens, first + 1, at)?;
    if is_unquoted_word(&tokens[first], "update") {
        return Some(after_first);
    }
    let expected = if is_unquoted_word(&tokens[first], "insert")
        || is_unquoted_word(&tokens[first], "merge")
    {
        "into"
    } else if is_unquoted_word(&tokens[first], "delete") {
        "from"
    } else {
        return None;
    };
    if !is_unquoted_word(&tokens[after_first], expected) {
        return None;
    }
    next_significant(tokens, after_first + 1, at)
}

fn is_qualified_name(tokens: &[TokenWithSpan], start: usize, end: usize) -> bool {
    let mut expect_word = true;
    let mut consumed = false;
    for token in &tokens[start..end] {
        if matches!(token.token, Token::Whitespace(_)) {
            continue;
        }
        if expect_word {
            if !matches!(token.token, Token::Word(_)) {
                return false;
            }
            consumed = true;
        } else if token.token != Token::Period {
            return false;
        }
        expect_word = !expect_word;
    }
    consumed && !expect_word
}

fn is_branch_identifier(token: &TokenWithSpan, dialect: &dyn Dialect) -> bool {
    let Token::Word(word) = &token.token else {
        return false;
    };
    match word.quote_style {
        Some('"') => !word.value.is_empty(),
        Some(_) => false,
        None => !dialect.is_reserved_for_identifier(word.keyword),
    }
}

fn normalize_iceberg_branch_references(tokens: &mut [TokenWithSpan], dialect: &dyn Dialect) {
    for at in 0..tokens.len() {
        if tokens[at].token != Token::AtSign {
            continue;
        }
        let Some(target) = dml_target_start(tokens, at) else {
            continue;
        };
        if !is_qualified_name(tokens, target, at) {
            continue;
        }
        let Some(branch) = next_significant(tokens, at + 1, tokens.len()) else {
            continue;
        };
        if !is_branch_identifier(&tokens[branch], dialect) {
            continue;
        }
        tokens[at].token = Token::Whitespace(Whitespace::Space);
        tokens[branch].token = Token::Whitespace(Whitespace::Space);
    }
}

fn is_digit_sequence(value: &str, predicate: impl Fn(char) -> bool) -> bool {
    let mut expect_digit = true;
    for character in value.chars() {
        if expect_digit {
            if !predicate(character) {
                return false;
            }
        } else if character != '_' && !predicate(character) {
            return false;
        }
        expect_digit = character == '_';
    }
    !expect_digit && !value.is_empty()
}

fn is_trino_non_decimal_integer(word: &str) -> bool {
    let Some((prefix, digits)) = word.split_at_checked(1) else {
        return false;
    };
    match prefix.to_ascii_uppercase().as_str() {
        "X" => is_digit_sequence(digits, |character| character.is_ascii_hexdigit()),
        "O" => is_digit_sequence(digits, |character| matches!(character, '0'..='7')),
        "B" => is_digit_sequence(digits, |character| matches!(character, '0' | '1')),
        _ => false,
    }
}

fn normalize_trino_non_decimal_integer_literals(tokens: &mut [TokenWithSpan]) {
    for index in 0..tokens.len().saturating_sub(1) {
        let (left, right) = (&tokens[index], &tokens[index + 1]);
        let is_literal = matches!(
            (&left.token, &right.token),
            (Token::Number(value, false), Token::Word(word))
                if value == "0"
                    && word.quote_style.is_none()
                    && left.span.end == right.span.start
                    && is_trino_non_decimal_integer(&word.value)
        );
        if is_literal {
            tokens[index + 1].token = Token::Whitespace(Whitespace::Space);
        }
    }
}

fn normalize_between_symmetry(tokens: &mut [TokenWithSpan]) {
    for modifier in 0..tokens.len() {
        if !is_unquoted_word(&tokens[modifier], "symmetric")
            && !is_unquoted_word(&tokens[modifier], "asymmetric")
        {
            continue;
        }
        if previous_significant(tokens, modifier)
            .is_some_and(|between| is_unquoted_word(&tokens[between], "between"))
        {
            tokens[modifier].token = Token::Whitespace(Whitespace::Space);
        }
    }
}

fn normalize_trim_without_character(tokens: &mut Vec<TokenWithSpan>) {
    for trim in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[trim], "trim") {
            continue;
        }
        let Some(open) = next_significant(tokens, trim + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(specification) = next_significant(tokens, open + 1, tokens.len()) else {
            continue;
        };
        if !["both", "leading", "trailing"]
            .iter()
            .any(|word| is_unquoted_word(&tokens[specification], word))
        {
            continue;
        }
        let Some(from) = next_significant(tokens, specification + 1, tokens.len()) else {
            continue;
        };
        if is_unquoted_word(&tokens[from], "from") {
            tokens.insert(
                from,
                token_with_span(
                    Token::SingleQuotedString(" ".to_string()),
                    &tokens[specification],
                ),
            );
        }
    }
}

fn normalize_empty_lambdas(tokens: &mut Vec<TokenWithSpan>) {
    for open in (0..tokens.len()).rev() {
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        if next_significant(tokens, open + 1, close).is_some() {
            continue;
        }
        let Some(arrow) = next_significant(tokens, close + 1, tokens.len()) else {
            continue;
        };
        if tokens[arrow].token == Token::Arrow {
            tokens.insert(
                close,
                word_with_span(&tokens[open], "__trino_empty_lambda", Keyword::NoKeyword),
            );
        }
    }
}

fn is_method_receiver_end(token: &TokenWithSpan) -> bool {
    matches!(
        token.token,
        Token::RParen
            | Token::RBracket
            | Token::SingleQuotedString(_)
            | Token::UnicodeStringLiteral(_)
            | Token::Number(_, _)
    )
}

fn normalize_method_calls(tokens: &mut Vec<TokenWithSpan>) {
    for separator in (0..tokens.len()).rev() {
        if tokens[separator].token == Token::DoubleColon {
            let Some(receiver) = previous_significant(tokens, separator) else {
                continue;
            };
            let Some(method) = next_significant(tokens, separator + 1, tokens.len()) else {
                continue;
            };
            let Some(open) = next_significant(tokens, method + 1, tokens.len()) else {
                continue;
            };
            if is_identifier(&tokens[receiver])
                && is_identifier(&tokens[method])
                && tokens[open].token == Token::LParen
            {
                tokens[separator].token = Token::Period;
                if let Token::Word(word) = &mut tokens[method].token {
                    word.quote_style = Some('"');
                    word.keyword = Keyword::NoKeyword;
                }
            }
            continue;
        }
        if tokens[separator].token != Token::Period {
            continue;
        }
        let Some(receiver) = previous_significant(tokens, separator) else {
            continue;
        };
        let Some(method) = next_significant(tokens, separator + 1, tokens.len()) else {
            continue;
        };
        let Some(open) = next_significant(tokens, method + 1, tokens.len()) else {
            continue;
        };
        if is_method_receiver_end(&tokens[receiver])
            && is_identifier(&tokens[method])
            && tokens[open].token == Token::LParen
        {
            tokens[separator].token = Token::Plus;
            if let Some(close) = matching_rparen(tokens, open) {
                if next_significant(tokens, open + 1, close).is_none() {
                    tokens.insert(
                        close,
                        word_with_span(&tokens[method], "NULL", Keyword::NULL),
                    );
                }
            }
        }
    }
}

fn normalize_pattern_processing_modes(tokens: &mut [TokenWithSpan]) {
    for mode in 0..tokens.len() {
        if !is_unquoted_word(&tokens[mode], "running") && !is_unquoted_word(&tokens[mode], "final")
        {
            continue;
        }
        let Some(function) = next_significant(tokens, mode + 1, tokens.len()) else {
            continue;
        };
        let Some(open) = next_significant(tokens, function + 1, tokens.len()) else {
            continue;
        };
        let valid_function = matches!(
            &tokens[function].token,
            Token::Word(word)
                if word.quote_style == Some('"')
                    || (word.quote_style.is_none()
                        && !trino_keywords::is_reserved_word(&word.value))
        );
        let parser_special = matches!(
            &tokens[function].token,
            Token::Word(word)
                if ["if", "nullif", "coalesce", "try", "format"]
                    .iter()
                    .any(|name| word.value.eq_ignore_ascii_case(name))
        );
        if valid_function && !parser_special && tokens[open].token == Token::LParen {
            tokens[mode].token = Token::Whitespace(Whitespace::Space);
        }
    }
}

fn is_row_pattern_clause(tokens: &[TokenWithSpan], pattern: usize) -> bool {
    let Some(container) = enclosing_parentheses(tokens, pattern).last().copied() else {
        return false;
    };
    let Some(introducer) = previous_significant(tokens, container) else {
        return false;
    };
    if is_unquoted_word(&tokens[introducer], "match_recognize")
        || is_unquoted_word(&tokens[introducer], "over")
    {
        return true;
    }
    if !is_unquoted_word(&tokens[introducer], "as") {
        return false;
    }
    let start = statement_start(tokens, introducer);
    (start..introducer).rev().any(|index| {
        is_unquoted_word(&tokens[index], "window")
            && enclosing_parentheses(tokens, index).len()
                == enclosing_parentheses(tokens, introducer).len()
    })
}

fn normalize_open_pattern_quantifiers(tokens: &mut [TokenWithSpan]) {
    let mut quantifiers = Vec::new();
    for pattern in 0..tokens.len() {
        if !is_unquoted_word(&tokens[pattern], "pattern") {
            continue;
        }
        if !is_row_pattern_clause(tokens, pattern) {
            continue;
        }
        let Some(open) = next_significant(tokens, pattern + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        for brace in open + 1..close {
            if tokens[brace].token != Token::LBrace {
                continue;
            }
            let Some(comma) = next_significant(tokens, brace + 1, close) else {
                continue;
            };
            let Some(right) = next_significant(tokens, comma + 1, close) else {
                continue;
            };
            if tokens[comma].token == Token::Comma && tokens[right].token == Token::RBrace {
                quantifiers.push((brace, right));
            }
        }
    }
    for (brace, right) in quantifiers {
        tokens[brace].token = Token::Mul;
        blank_non_whitespace(tokens, brace + 1, right + 1);
    }
}

fn normalize_over_projection_alias(tokens: &mut [TokenWithSpan]) {
    for over in 0..tokens.len() {
        if !is_unquoted_word(&tokens[over], "over") {
            continue;
        }
        let Some(previous) = previous_significant(tokens, over) else {
            continue;
        };
        let Some(next) = next_significant(tokens, over + 1, tokens.len()) else {
            continue;
        };
        if tokens[previous].token == Token::RParen && is_unquoted_word(&tokens[next], "from") {
            replace_word(&mut tokens[over], "over", Keyword::NoKeyword);
        }
    }
}

fn valid_unicode_escape_character(value: &str) -> Option<char> {
    let mut chars = value.chars();
    let character = chars.next()?;
    if chars.next().is_some()
        || character.is_ascii_hexdigit()
        || character == '+'
        || character == '\''
        || character == '"'
        || character.is_whitespace()
    {
        return None;
    }
    Some(character)
}

fn validate_unicode_string(value: &str, escape: char) -> bool {
    let chars = value.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != escape {
            index += 1;
            continue;
        }
        if chars.get(index + 1) == Some(&escape) {
            index += 2;
            continue;
        }
        let (digits, start) = if chars.get(index + 1) == Some(&'+') {
            (6, index + 2)
        } else {
            (4, index + 1)
        };
        let end = start + digits;
        if end > chars.len()
            || !chars[start..end]
                .iter()
                .all(|character| character.is_ascii_hexdigit())
        {
            return false;
        }
        let codepoint = chars[start..end]
            .iter()
            .collect::<String>()
            .chars()
            .fold(0u32, |value, character| {
                value * 16 + character.to_digit(16).unwrap_or(0)
            });
        if char::from_u32(codepoint).is_none() || (0xD800..=0xDFFF).contains(&codepoint) {
            return false;
        }
        index = end;
    }
    true
}

fn normalize_unicode_escapes(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for unicode in 0..tokens.len() {
        let Token::UnicodeStringLiteral(value) = &tokens[unicode].token else {
            continue;
        };
        let value = value.clone();
        let mut escape_clause = None;
        if let Some(uescape) = next_significant(tokens, unicode + 1, tokens.len()) {
            if is_unquoted_word(&tokens[uescape], "uescape") {
                let literal =
                    next_significant(tokens, uescape + 1, tokens.len()).ok_or_else(|| {
                        syntax_error(&tokens[uescape], "UESCAPE requires a character literal")
                    })?;
                let Token::SingleQuotedString(character) = &tokens[literal].token else {
                    return Err(syntax_error(
                        &tokens[literal],
                        "UESCAPE requires a character literal",
                    ));
                };
                let escape = valid_unicode_escape_character(character).ok_or_else(|| {
                    syntax_error(&tokens[literal], "invalid Unicode escape character")
                })?;
                escape_clause = Some((uescape, literal, escape));
            }
        }
        if let Some((uescape, literal, escape)) = escape_clause {
            if !validate_unicode_string(&value, escape) {
                return Err(syntax_error(
                    &tokens[unicode],
                    "invalid Unicode escape sequence",
                ));
            }
            blank_non_whitespace(tokens, uescape, literal + 1);
        }
    }
    Ok(())
}

fn validate_trino_identifiers(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    for token in tokens {
        if matches!(
            &token.token,
            Token::Word(word) if word.quote_style == Some('"') && word.value.is_empty()
        ) {
            return Err(ParserError::ParserError(format!(
                "Zero-length delimited identifier not allowed at Line: {}, Column: {}",
                token.span.start.line, token.span.start.column
            )));
        }
    }
    for pair in tokens.windows(2) {
        if matches!(pair[0].token, Token::Number(_, _))
            && matches!(pair[1].token, Token::Word(_))
            && pair[0].span.end == pair[1].span.start
        {
            return Err(ParserError::ParserError(format!(
                "identifiers must not start with a digit at Line: {}, Column: {}",
                pair[0].span.start.line, pair[0].span.start.column
            )));
        }
    }
    Ok(())
}

fn is_in_create_type_declaration(tokens: &[TokenWithSpan], index: usize) -> bool {
    let start = statement_start(tokens, index);
    let Some(create) = (start..index)
        .rev()
        .find(|current| unquoted_keyword(&tokens[*current]) == Some(Keyword::CREATE))
    else {
        return false;
    };
    let mut cursor = create + 1;
    let declaration = loop {
        let Some(current) = next_significant(tokens, cursor, index) else {
            return false;
        };
        if matches!(
            unquoted_keyword(&tokens[current]),
            Some(Keyword::TABLE | Keyword::VIEW | Keyword::FUNCTION)
        ) {
            break current;
        }
        cursor = current + 1;
    };
    let Some(open) =
        (declaration + 1..index).find(|current| tokens[*current].token == Token::LParen)
    else {
        return false;
    };
    matching_rparen(tokens, open).is_some_and(|close| close > index)
}

fn is_routine_return_type_position(tokens: &[TokenWithSpan], index: usize) -> bool {
    let start = statement_start(tokens, index);
    let Some(returns) = (start..index)
        .rev()
        .find(|current| unquoted_keyword(&tokens[*current]) == Some(Keyword::RETURNS))
    else {
        return false;
    };
    let mut depth = 0usize;
    for token in tokens.iter().take(index).skip(returns + 1) {
        match token.token {
            Token::LParen | Token::LBracket | Token::Lt => depth += 1,
            Token::RParen | Token::RBracket | Token::Gt if depth > 0 => depth -= 1,
            _ if depth == 0
                && [
                    "as",
                    "begin",
                    "called",
                    "comment",
                    "deterministic",
                    "language",
                    "not",
                    "return",
                    "security",
                ]
                .iter()
                .any(|word| is_unquoted_word(token, word)) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

fn is_drop_function_signature_position(tokens: &[TokenWithSpan], index: usize) -> bool {
    let start = statement_start(tokens, index);
    let Some(drop) = next_significant(tokens, start, index) else {
        return false;
    };
    let Some(function) = next_significant(tokens, drop + 1, index) else {
        return false;
    };
    if unquoted_keyword(&tokens[drop]) != Some(Keyword::DROP)
        || unquoted_keyword(&tokens[function]) != Some(Keyword::FUNCTION)
    {
        return false;
    }
    let Some(open) = (function + 1..index).find(|current| tokens[*current].token == Token::LParen)
    else {
        return false;
    };
    matching_rparen(tokens, open).is_some_and(|close| close > index)
}

fn is_type_context(tokens: &[TokenWithSpan], index: usize) -> bool {
    let previous_is_type_marker = previous_significant(tokens, index).is_some_and(|previous| {
        matches!(
            unquoted_keyword(&tokens[previous]),
            Some(Keyword::AS | Keyword::RETURNS)
        )
    });
    previous_is_type_marker
        || is_in_create_type_declaration(tokens, index)
        || statement_starts_with(tokens, index, Keyword::ALTER)
}

fn is_cast_type_position(tokens: &[TokenWithSpan], index: usize) -> bool {
    for open in enclosing_parentheses(tokens, index).into_iter().rev() {
        let Some(function) = previous_significant(tokens, open) else {
            continue;
        };
        if !is_unquoted_word(&tokens[function], "cast")
            && !is_unquoted_word(&tokens[function], "try_cast")
        {
            continue;
        }
        let mut depth = 0usize;
        for token in tokens.iter().take(index).skip(open + 1) {
            match token.token {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace if depth > 0 => depth -= 1,
                _ if depth == 0 && is_unquoted_word(token, "as") => return true,
                _ => {}
            }
        }
    }
    false
}

fn is_structural_type_position(tokens: &[TokenWithSpan], index: usize) -> bool {
    if is_type_context(tokens, index)
        || is_cast_type_position(tokens, index)
        || is_routine_return_type_position(tokens, index)
        || is_drop_function_signature_position(tokens, index)
    {
        return true;
    }
    enclosing_parentheses(tokens, index)
        .into_iter()
        .rev()
        .filter_map(|open| previous_significant(tokens, open))
        .any(|container| {
            (is_type_container(&tokens[container])
                || is_unquoted_word(&tokens[container], "struct"))
                && (is_type_context(tokens, container) || is_cast_type_position(tokens, container))
        })
}

fn source_type_ident(token: &TokenWithSpan) -> Option<Ident> {
    let Token::Word(word) = &token.token else {
        return None;
    };
    Some(match word.quote_style {
        Some(quote) => Ident::with_quote_and_span(quote, token.span, word.value.clone()),
        None => Ident::with_span(token.span, word.value.clone()),
    })
}

fn alter_type_starts(tokens: &[TokenWithSpan]) -> Vec<usize> {
    let mut starts = Vec::new();
    for index in 0..tokens.len() {
        if is_unquoted_word(&tokens[index], "add") {
            let statement = statement_start(tokens, index);
            if !(statement..index).any(|candidate| is_unquoted_word(&tokens[candidate], "alter")) {
                continue;
            }
            let Some(column) = next_significant(tokens, index + 1, tokens.len()) else {
                continue;
            };
            if !is_unquoted_word(&tokens[column], "column") {
                continue;
            }
            let mut name = next_significant(tokens, column + 1, tokens.len());
            if name.is_some_and(|current| is_unquoted_word(&tokens[current], "if")) {
                name = name
                    .and_then(|current| next_significant(tokens, current + 1, tokens.len()))
                    .and_then(|current| next_significant(tokens, current + 1, tokens.len()))
                    .and_then(|current| next_significant(tokens, current + 1, tokens.len()));
            }
            if let Some(type_start) = name
                .and_then(|name| consume_qualified_name(tokens, name, tokens.len()))
                .and_then(|after| next_significant(tokens, after, tokens.len()))
            {
                starts.push(type_start);
            }
        }
        if is_unquoted_word(&tokens[index], "type") {
            let previous = previous_significant(tokens, index);
            let before_previous = previous.and_then(|value| previous_significant(tokens, value));
            if previous.is_some_and(|value| is_unquoted_word(&tokens[value], "data"))
                && before_previous.is_some_and(|value| is_unquoted_word(&tokens[value], "set"))
            {
                if let Some(type_start) = next_significant(tokens, index + 1, tokens.len()) {
                    starts.push(type_start);
                }
            }
        }
    }
    starts
}

fn collect_source_type_names(tokens: &[TokenWithSpan]) -> Vec<Ident> {
    let alter_starts = alter_type_starts(tokens);
    let mut names = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let Token::Word(word) = &token.token else {
            continue;
        };
        let lower = word.value.to_ascii_lowercase();
        if types::is_known_type(&lower) {
            continue;
        }
        let previous = previous_significant(tokens, index);
        let next = next_significant(tokens, index + 1, tokens.len());
        let explicit_marker = previous.is_some_and(|previous| {
            is_unquoted_word(&tokens[previous], "returning")
                || (is_unquoted_word(&tokens[previous], "returns")
                    && !(is_unquoted_word(token, "null")
                        && next.is_some_and(|next| is_unquoted_word(&tokens[next], "on"))))
        });
        let typed_literal = word.keyword == Keyword::NoKeyword
            && next.is_some_and(|next| is_single_quoted_string(&tokens[next]));
        let foreign_builtin = matches!(lower.as_str(), "int64" | "string" | "bytea")
            && is_structural_type_position(tokens, index)
            && !previous.is_some_and(|previous| is_unquoted_word(&tokens[previous], "default"))
            && !next.is_some_and(|next| is_probable_type_name(&tokens[next]));
        if explicit_marker || typed_literal || foreign_builtin || alter_starts.contains(&index) {
            if let Some(ident) = source_type_ident(token) {
                names.push(ident);
            }
        }
    }
    names
}

fn is_trino_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Value(value) => matches!(
            value.value,
            Value::Number(_, _)
                | Value::SingleQuotedString(_)
                | Value::UnicodeStringLiteral(_)
                | Value::HexStringLiteral(_)
                | Value::Boolean(_)
                | Value::Null
        ),
        Expr::TypedString(_) => true,
        Expr::Interval(interval) => is_trino_interval_literal(interval),
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => {
            matches!(expr.as_ref(), Expr::Value(value) if matches!(value.value, Value::Number(_, _)))
        }
        _ => false,
    }
}

pub(super) fn is_trino_interval_literal(interval: &Interval) -> bool {
    is_trino_interval_value(interval)
        && matches!(
            (interval.leading_field.clone(), interval.last_field.clone()),
            (None, None)
                | (Some(DateTimeField::Year), None | Some(DateTimeField::Month))
                | (Some(DateTimeField::Month), None)
                | (
                    Some(DateTimeField::Day),
                    None | Some(DateTimeField::Hour)
                        | Some(DateTimeField::Minute)
                        | Some(DateTimeField::Second)
                )
                | (
                    Some(DateTimeField::Hour),
                    None | Some(DateTimeField::Minute) | Some(DateTimeField::Second)
                )
                | (
                    Some(DateTimeField::Minute),
                    None | Some(DateTimeField::Second)
                )
                | (Some(DateTimeField::Second), None)
        )
}

fn is_trino_interval_expression(interval: &Interval) -> bool {
    is_trino_interval_value(interval)
        && (interval.last_field.is_none() || is_trino_interval_literal(interval))
}

fn is_trino_interval_value(interval: &Interval) -> bool {
    match interval.value.as_ref() {
        Expr::Value(value) => matches!(
            value.value,
            Value::SingleQuotedString(_) | Value::UnicodeStringLiteral(_)
        ),
        Expr::UnaryOp {
            op: UnaryOperator::Plus | UnaryOperator::Minus,
            expr,
        } => matches!(
            expr.as_ref(),
            Expr::Value(value)
                if matches!(
                    value.value,
                    Value::SingleQuotedString(_) | Value::UnicodeStringLiteral(_)
                )
        ),
        _ => false,
    }
}

fn is_trino_row_count(expr: &Expr) -> bool {
    matches!(expr, Expr::Value(value) if match &value.value {
        Value::Placeholder(value) => value == "?",
        Value::Number(value, _) => !value.contains(['.', 'e', 'E']),
        _ => false,
    })
}

fn is_trino_table_sample(sample: &TableSampleKind) -> bool {
    let sample = match sample {
        TableSampleKind::BeforeTableAlias(sample) | TableSampleKind::AfterTableAlias(sample) => {
            sample
        }
    };
    matches!(
        sample.name,
        Some(TableSampleMethod::Bernoulli | TableSampleMethod::System)
    ) && sample
        .quantity
        .as_ref()
        .is_some_and(|quantity| quantity.parenthesized && quantity.unit.is_none())
        && sample.bucket.is_none()
        && sample.seed.is_none()
        && sample.offset.is_none()
}

fn alias_token_location(ident: &Ident, tokens: &[TokenWithSpan]) -> Option<(u64, u64)> {
    if ident.span.start.line != 0 {
        return Some((ident.span.start.line, ident.span.start.column));
    }
    if ident.quote_style != Some('\'') {
        return None;
    }
    tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let Token::SingleQuotedString(value) = &token.token else {
                return None;
            };
            if value != &ident.value {
                return None;
            }
            let previous = previous_significant(tokens, index);
            let next = next_significant(tokens, index + 1, tokens.len());
            let adjacent_as = previous
                .is_some_and(|previous| unquoted_keyword(&tokens[previous]) == Some(Keyword::AS))
                || next.is_some_and(|next| unquoted_keyword(&tokens[next]) == Some(Keyword::AS));
            let follows_value = previous.is_some_and(|previous| {
                matches!(
                    &tokens[previous].token,
                    Token::Number(_, _)
                        | Token::SingleQuotedString(_)
                        | Token::DoubleQuotedString(_)
                        | Token::RParen
                        | Token::RBracket
                        | Token::RBrace
                ) || matches!(
                    &tokens[previous].token,
                    Token::Word(word)
                        if word.quote_style.is_some()
                            || !matches!(
                                word.keyword,
                                Keyword::AS
                                    | Keyword::BY
                                    | Keyword::ELSE
                                    | Keyword::FROM
                                    | Keyword::JOIN
                                    | Keyword::ON
                                    | Keyword::SELECT
                                    | Keyword::THEN
                                    | Keyword::WHEN
                                    | Keyword::WHERE
                                    | Keyword::WITH
                            )
                )
            });
            let ends_alias = next.map_or(true, |next| {
                matches!(
                    tokens[next].token,
                    Token::Comma | Token::EOF | Token::RParen | Token::SemiColon
                ) || matches!(
                    unquoted_keyword(&tokens[next]),
                    Some(
                        Keyword::EXCEPT
                            | Keyword::FETCH
                            | Keyword::FROM
                            | Keyword::GROUP
                            | Keyword::HAVING
                            | Keyword::INTERSECT
                            | Keyword::LIMIT
                            | Keyword::OFFSET
                            | Keyword::ORDER
                            | Keyword::UNION
                            | Keyword::WHERE
                            | Keyword::WINDOW
                    )
                )
            });
            let score = usize::from(adjacent_as) * 4
                + usize::from(follows_value) * 2
                + usize::from(ends_alias);
            Some((score, index, token.span.start))
        })
        .max_by_key(|(score, index, _)| (*score, std::cmp::Reverse(*index)))
        .map(|(_, _, location)| (location.line, location.column))
}

fn invalid_alias_error(ident: &Ident, tokens: &[TokenWithSpan]) -> Option<ParserError> {
    let message = match ident.quote_style {
        None if trino_keywords::is_reserved_word(&ident.value) => format!(
            "reserved keyword '{}' cannot be used as an unquoted Trino alias",
            ident.value
        ),
        Some('\'') => "single-quoted aliases are not supported by Trino".to_string(),
        Some('"') | None => return None,
        Some(_) => "invalid quoted alias for Trino".to_string(),
    };
    let message = if let Some((line, column)) = alias_token_location(ident, tokens) {
        format!("{message} at Line: {line}, Column: {column}")
    } else {
        message
    };
    Some(ParserError::ParserError(message))
}

fn is_identifier_typed_literal(expr: &Expr, alias: &Ident, tokens: &[TokenWithSpan]) -> bool {
    let Expr::Identifier(type_name) = expr else {
        return false;
    };
    if alias.quote_style != Some('\'') {
        return false;
    }
    tokens.iter().enumerate().any(|(index, token)| {
        matches!(&token.token, Token::SingleQuotedString(value) if value == &alias.value)
            && previous_significant(tokens, index).is_some_and(|previous| {
                matches!(&tokens[previous].token, Token::Word(word) if word.value == type_name.value)
                    && tokens[previous].span == type_name.span
            })
    })
}

fn validate_trino_aliases(
    statements: &[Statement],
    tokens: &[TokenWithSpan],
) -> Result<(), ParserError> {
    struct AliasVisitor<'a> {
        tokens: &'a [TokenWithSpan],
        error: Option<ParserError>,
    }

    impl AliasVisitor<'_> {
        fn validate_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
            if let Some(error) = invalid_alias_error(ident, self.tokens) {
                self.error = Some(error);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }

        fn validate_table_alias(&mut self, alias: &TableAlias) -> ControlFlow<()> {
            if self.validate_ident(&alias.name).is_break() {
                return ControlFlow::Break(());
            }
            for column in &alias.columns {
                if self.validate_ident(&column.name).is_break() {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        }
    }

    impl Visitor for AliasVisitor<'_> {
        type Break = ();

        fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
            if let Some(with) = &query.with {
                for cte in &with.cte_tables {
                    if self.validate_table_alias(&cte.alias).is_break() {
                        return ControlFlow::Break(());
                    }
                }
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
            for item in &select.projection {
                match item {
                    SelectItem::ExprWithAlias { expr, alias } => {
                        let typed_literal = is_identifier_typed_literal(expr, alias, self.tokens);
                        if !typed_literal && self.validate_ident(alias).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                    SelectItem::ExprWithAliases { aliases, .. } => {
                        for alias in aliases {
                            if self.validate_ident(alias).is_break() {
                                return ControlFlow::Break(());
                            }
                        }
                    }
                    SelectItem::QualifiedWildcard(_, options) | SelectItem::Wildcard(options) => {
                        if let Some(alias) = &options.opt_alias {
                            if self.validate_ident(alias).is_break() {
                                return ControlFlow::Break(());
                            }
                        }
                    }
                    SelectItem::UnnamedExpr(_) => {}
                }
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_table_factor(
            &mut self,
            table_factor: &TableFactor,
        ) -> ControlFlow<Self::Break> {
            match table_factor {
                TableFactor::Pivot {
                    aggregate_functions,
                    ..
                } => {
                    for expression in aggregate_functions {
                        if let Some(alias) = &expression.alias {
                            if self.validate_ident(alias).is_break() {
                                return ControlFlow::Break(());
                            }
                        }
                    }
                }
                TableFactor::Unpivot { columns, .. } => {
                    for expression in columns {
                        if let Some(alias) = &expression.alias {
                            if self.validate_ident(alias).is_break() {
                                return ControlFlow::Break(());
                            }
                        }
                    }
                }
                TableFactor::MatchRecognize { measures, .. } => {
                    for measure in measures {
                        if self.validate_ident(&measure.alias).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                }
                _ => {}
            }
            let alias = match table_factor {
                TableFactor::Table { alias, .. }
                | TableFactor::Derived { alias, .. }
                | TableFactor::TableFunction { alias, .. }
                | TableFactor::Function { alias, .. }
                | TableFactor::UNNEST { alias, .. }
                | TableFactor::JsonTable { alias, .. }
                | TableFactor::OpenJsonTable { alias, .. }
                | TableFactor::NestedJoin { alias, .. }
                | TableFactor::Pivot { alias, .. }
                | TableFactor::Unpivot { alias, .. }
                | TableFactor::MatchRecognize { alias, .. }
                | TableFactor::XmlTable { alias, .. }
                | TableFactor::SemanticView { alias, .. } => alias,
            };
            if let Some(alias) = alias {
                if self.validate_table_alias(alias).is_break() {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        }
    }

    let mut visitor = AliasVisitor {
        tokens,
        error: None,
    };
    let _ = statements.to_vec().visit(&mut visitor);
    match visitor.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn source_ident_index(tokens: &[TokenWithSpan], ident: &Ident) -> Option<usize> {
    tokens.iter().position(|token| {
        token.span.start == ident.span.start
            && matches!(
                &token.token,
                Token::Word(word)
                    if word.value == ident.value && word.quote_style == ident.quote_style
            )
    })
}

fn validate_trino_ast(
    statements: &[Statement],
    source_tokens: &[TokenWithSpan],
) -> Result<(), ParserError> {
    let mut error = None;
    struct GrammarVisitor<'a> {
        error: &'a mut Option<ParserError>,
        source_tokens: &'a [TokenWithSpan],
    }
    impl GrammarVisitor<'_> {
        fn reject(&mut self, message: &str) -> ControlFlow<()> {
            *self.error = Some(ParserError::ParserError(message.to_string()));
            ControlFlow::Break(())
        }

        fn reject_at(&mut self, message: &str, line: u64, column: u64) -> ControlFlow<()> {
            *self.error = Some(ParserError::ParserError(format!(
                "{message} at Line: {line}, Column: {column}"
            )));
            ControlFlow::Break(())
        }

        fn reject_at_ident(&mut self, message: &str, ident: &Ident) -> ControlFlow<()> {
            self.reject_at(message, ident.span.start.line, ident.span.start.column)
        }

        fn reject_at_token(&mut self, message: &str, token: &TokenWithSpan) -> ControlFlow<()> {
            self.reject_at(message, token.span.start.line, token.span.start.column)
        }

        fn validate_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
            if ident.quote_style.is_none() && trino_keywords::is_reserved_word(&ident.value) {
                return self.reject_at_ident(
                    &format!(
                        "reserved keyword '{}' cannot be used as an unquoted Trino identifier",
                        ident.value
                    ),
                    ident,
                );
            }
            ControlFlow::Continue(())
        }

        fn validate_object_name(&mut self, name: &ObjectName) -> ControlFlow<()> {
            for ident in name.0.iter().filter_map(|part| part.as_ident()) {
                if self.validate_ident(ident).is_break() {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        }

        fn validate_data_type(&mut self, data_type: &DataType) -> ControlFlow<()> {
            match data_type {
                DataType::Struct(fields, _) | DataType::Tuple(fields) => {
                    for field in fields {
                        if let Some(name) = &field.field_name {
                            if self.validate_ident(name).is_break() {
                                return ControlFlow::Break(());
                            }
                        }
                        if self.validate_data_type(&field.field_type).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                }
                DataType::Array(element) => {
                    let element = match element {
                        ArrayElemTypeDef::None => None,
                        ArrayElemTypeDef::AngleBracket(element)
                        | ArrayElemTypeDef::SquareBracket(element, _)
                        | ArrayElemTypeDef::Parenthesis(element) => Some(element.as_ref()),
                    };
                    if element.is_some_and(|element| self.validate_data_type(element).is_break()) {
                        return ControlFlow::Break(());
                    }
                }
                DataType::Map(key, value) => {
                    if self.validate_data_type(key).is_break()
                        || self.validate_data_type(value).is_break()
                    {
                        return ControlFlow::Break(());
                    }
                }
                DataType::Nested(columns) => {
                    for column in columns {
                        if self.validate_ident(&column.name).is_break()
                            || self.validate_data_type(&column.data_type).is_break()
                        {
                            return ControlFlow::Break(());
                        }
                    }
                }
                DataType::Nullable(inner) | DataType::LowCardinality(inner)
                    if self.validate_data_type(inner).is_break() =>
                {
                    return ControlFlow::Break(());
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }

        fn validate_expression_function_name(&mut self, name: &ObjectName) -> ControlFlow<()> {
            if name.0.len() != 1 {
                return self.validate_object_name(name);
            }
            let Some(ident) = name.0.first().and_then(|part| part.as_ident()) else {
                return ControlFlow::Continue(());
            };
            let syntax_function = matches!(
                ident.value.to_ascii_lowercase().as_str(),
                "current_date"
                    | "current_catalog"
                    | "current_path"
                    | "current_schema"
                    | "current_time"
                    | "current_timestamp"
                    | "current_user"
                    | "grouping"
                    | "json_array"
                    | "json_exists"
                    | "json_object"
                    | "json_query"
                    | "json_value"
                    | "listagg"
                    | "localtime"
                    | "localtimestamp"
                    | "normalize"
                    | "rollup"
                    | "session_user"
                    | "system_user"
                    | "table"
            );
            if !syntax_function && self.validate_ident(ident).is_break() {
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        }

        fn validate_match_measure_alias(&mut self, alias: &Ident) -> ControlFlow<()> {
            let explicit_as = source_ident_index(self.source_tokens, alias)
                .and_then(|alias| previous_significant(self.source_tokens, alias))
                .is_some_and(|previous| is_unquoted_word(&self.source_tokens[previous], "as"));
            if !explicit_as {
                return self.reject_at_ident("MATCH_RECOGNIZE measure aliases require AS", alias);
            }
            ControlFlow::Continue(())
        }
    }
    impl Visitor for GrammarVisitor<'_> {
        type Break = ();

        fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
            match statement {
                Statement::Call(function) => {
                    if self.validate_object_name(&function.name).is_break() {
                        return ControlFlow::Break(());
                    }
                    let valid_argument = |argument: &FunctionArg| match argument {
                        FunctionArg::Unnamed(FunctionArgExpr::Expr(_)) => true,
                        FunctionArg::Named { arg, operator, .. } => {
                            matches!(arg, FunctionArgExpr::Expr(_))
                                && *operator == FunctionArgOperator::RightArrow
                        }
                        _ => false,
                    };
                    let valid_list = match &function.args {
                        FunctionArguments::List(arguments) => {
                            arguments.duplicate_treatment.is_none()
                                && arguments.clauses.is_empty()
                                && arguments.args.iter().all(valid_argument)
                        }
                        _ => false,
                    };
                    if !matches!(function.parameters, FunctionArguments::None)
                        || !valid_list
                        || function.filter.is_some()
                        || function.null_treatment.is_some()
                        || function.over.is_some()
                        || !function.within_group.is_empty()
                    {
                        return self.reject("invalid Trino CALL syntax");
                    }
                }
                Statement::CreateIndex(_) => {
                    return self.reject("CREATE INDEX is not supported by Trino");
                }
                Statement::Update(update) => {
                    if update.from.is_some() || update.returning.is_some() {
                        return self.reject("invalid Trino UPDATE syntax");
                    }
                }
                Statement::Delete(delete) => {
                    if !matches!(delete.from, FromTable::WithFromKeyword(_))
                        || delete.using.is_some()
                        || delete.returning.is_some()
                    {
                        return self.reject_at_token(
                            "invalid Trino DELETE syntax",
                            &delete.delete_token.0,
                        );
                    }
                }
                Statement::Insert(insert) => {
                    if !insert.into || insert.returning.is_some() {
                        return self.reject_at_token(
                            "invalid Trino INSERT syntax",
                            &insert.insert_token.0,
                        );
                    }
                }
                Statement::Truncate(truncate) => {
                    if !truncate.table {
                        if let Some(ident) = truncate
                            .table_names
                            .first()
                            .and_then(|target| target.name.0.first())
                            .and_then(|part| part.as_ident())
                        {
                            return self.reject_at_ident("Trino TRUNCATE requires TABLE", ident);
                        }
                        return self.reject("Trino TRUNCATE requires TABLE");
                    }
                }
                Statement::Deallocate { name, prepare } => {
                    if self.validate_ident(name).is_break() {
                        return ControlFlow::Break(());
                    }
                    if !prepare {
                        return self.reject_at_ident("Trino DEALLOCATE requires PREPARE", name);
                    }
                }
                Statement::Execute {
                    name,
                    parameters,
                    has_parentheses,
                    immediate,
                    into,
                    output,
                    default,
                    ..
                } => {
                    if let Some(name) = name {
                        if self.validate_object_name(name).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                    let valid = if *immediate {
                        name.is_none() && !has_parentheses
                    } else {
                        name.is_some() && parameters.is_empty() && !has_parentheses
                    };
                    if !valid || !into.is_empty() || *output || *default {
                        return self.reject("invalid Trino EXECUTE syntax");
                    }
                }
                Statement::CreateTable(create) => {
                    for column in &create.columns {
                        let table_like = column.name.quote_style.is_none()
                            && column.name.value.eq_ignore_ascii_case("like");
                        if !table_like && self.validate_ident(&column.name).is_break() {
                            return ControlFlow::Break(());
                        }
                        if self.validate_data_type(&column.data_type).is_break() {
                            return ControlFlow::Break(());
                        }
                        for option in &column.options {
                            if let ColumnOption::Default(expr) = &option.option {
                                if !is_trino_literal(expr) {
                                    return self.reject("Trino column DEFAULT requires a literal");
                                }
                            }
                        }
                    }
                }
                Statement::CreateView(create) => {
                    if self.validate_object_name(&create.name).is_break() {
                        return ControlFlow::Break(());
                    }
                    for column in &create.columns {
                        if self.validate_ident(&column.name).is_break() {
                            return ControlFlow::Break(());
                        }
                        if column
                            .data_type
                            .as_ref()
                            .is_some_and(|data_type| self.validate_data_type(data_type).is_break())
                        {
                            return ControlFlow::Break(());
                        }
                    }
                }
                Statement::Drop { names, .. } => {
                    for name in names {
                        if self.validate_object_name(name).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                }
                Statement::CreateSchema { schema_name, .. } => match schema_name {
                    SchemaName::Simple(name) => {
                        if self.validate_object_name(name).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                    SchemaName::UnnamedAuthorization(authorization) => {
                        if self.validate_ident(authorization).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                    SchemaName::NamedAuthorization(name, authorization) => {
                        if self.validate_object_name(name).is_break()
                            || self.validate_ident(authorization).is_break()
                        {
                            return ControlFlow::Break(());
                        }
                    }
                },
                Statement::CreateFunction(create) => {
                    if self.validate_object_name(&create.name).is_break() {
                        return ControlFlow::Break(());
                    }
                    if let Some(arguments) = &create.args {
                        for argument in arguments {
                            if argument
                                .name
                                .as_ref()
                                .is_some_and(|name| self.validate_ident(name).is_break())
                                || self.validate_data_type(&argument.data_type).is_break()
                            {
                                return ControlFlow::Break(());
                            }
                        }
                    }
                    if let Some(return_type) = &create.return_type {
                        let data_type = match return_type {
                            FunctionReturnType::DataType(data_type)
                            | FunctionReturnType::SetOf(data_type) => data_type,
                        };
                        if self.validate_data_type(data_type).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                }
                Statement::Comment { object_name, .. } => {
                    if self.validate_object_name(object_name).is_break() {
                        return ControlFlow::Break(());
                    }
                }
                Statement::Prepare {
                    name, data_types, ..
                } => {
                    if self.validate_ident(name).is_break() {
                        return ControlFlow::Break(());
                    }
                    for data_type in data_types {
                        if self.validate_data_type(data_type).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
            let has_limit = matches!(
                query.limit_clause,
                Some(LimitClause::LimitOffset { limit: Some(_), .. })
                    | Some(LimitClause::OffsetCommaLimit { .. })
            );
            if has_limit && query.fetch.is_some() {
                if let Some(quantity) = query
                    .fetch
                    .as_ref()
                    .and_then(|fetch| fetch.quantity.as_ref())
                {
                    let start = quantity.span().start;
                    return self.reject_at(
                        "Trino query cannot contain both LIMIT and FETCH",
                        start.line,
                        start.column,
                    );
                }
                if let Some(fetch) = self
                    .source_tokens
                    .iter()
                    .find(|token| is_unquoted_word(token, "fetch"))
                    .cloned()
                {
                    return self.reject_at_token(
                        "Trino query cannot contain both LIMIT and FETCH",
                        &fetch,
                    );
                }
                return self.reject("Trino query cannot contain both LIMIT and FETCH");
            }
            if let Some(limit_clause) = &query.limit_clause {
                let valid = match limit_clause {
                    LimitClause::LimitOffset {
                        limit,
                        offset,
                        limit_by,
                    } => {
                        limit_by.is_empty()
                            && limit.as_ref().map_or(true, is_trino_row_count)
                            && offset
                                .as_ref()
                                .map_or(true, |offset| is_trino_row_count(&offset.value))
                    }
                    LimitClause::OffsetCommaLimit { .. } => false,
                };
                if !valid {
                    return self.reject("Trino LIMIT/OFFSET requires an integer or ?");
                }
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
            if select.qualify.is_some() {
                return self.reject("QUALIFY is not supported by Trino");
            }
            for table in &select.from {
                for join in &table.joins {
                    let constraint = match &join.join_operator {
                        JoinOperator::Join(constraint)
                        | JoinOperator::Inner(constraint)
                        | JoinOperator::Left(constraint)
                        | JoinOperator::LeftOuter(constraint)
                        | JoinOperator::Right(constraint)
                        | JoinOperator::RightOuter(constraint)
                        | JoinOperator::FullOuter(constraint) => constraint,
                        JoinOperator::CrossJoin(JoinConstraint::None) => continue,
                        JoinOperator::CrossJoin(_) => {
                            return self.reject("Trino CROSS JOIN cannot have join criteria");
                        }
                        _ => return self.reject("join type is not supported by Trino"),
                    };
                    match constraint {
                        JoinConstraint::None => {
                            if let TableFactor::Table { name, .. } = &join.relation {
                                if let Some(ident) = name.0.first().and_then(|part| part.as_ident())
                                {
                                    return self
                                        .reject_at_ident("Trino JOIN requires ON or USING", ident);
                                }
                            }
                            return self.reject("Trino JOIN requires ON or USING");
                        }
                        JoinConstraint::Using(columns)
                            if columns.is_empty()
                                || columns.iter().any(|column| column.0.len() != 1) =>
                        {
                            if let Some(ident) = columns
                                .iter()
                                .flat_map(|column| column.0.iter())
                                .find_map(|part| part.as_ident())
                            {
                                return self.reject_at_ident(
                                    "Trino JOIN USING requires unqualified column names",
                                    ident,
                                );
                            }
                            return self
                                .reject("Trino JOIN USING requires unqualified column names");
                        }
                        JoinConstraint::Using(columns) => {
                            for column in columns {
                                if self.validate_object_name(column).is_break() {
                                    return ControlFlow::Break(());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
            match expr {
                Expr::Identifier(ident) => {
                    let current_value = ident.quote_style.is_none()
                        && matches!(
                            ident.value.to_ascii_lowercase().as_str(),
                            "current_catalog"
                                | "current_date"
                                | "current_path"
                                | "current_schema"
                                | "current_time"
                                | "current_timestamp"
                                | "current_user"
                                | "localtime"
                                | "localtimestamp"
                                | "session_user"
                                | "system_user"
                        );
                    if !current_value && self.validate_ident(ident).is_break() {
                        return ControlFlow::Break(());
                    }
                }
                Expr::CompoundIdentifier(idents) => {
                    for ident in idents {
                        if self.validate_ident(ident).is_break() {
                            return ControlFlow::Break(());
                        }
                    }
                }
                Expr::Cast { data_type, .. } if self.validate_data_type(data_type).is_break() => {
                    return ControlFlow::Break(());
                }
                Expr::Interval(interval) if !is_trino_interval_expression(interval) => {
                    let start = interval.value.span().start;
                    return self.reject_at(
                        "invalid Trino interval literal",
                        start.line,
                        start.column,
                    );
                }
                _ => {}
            }
            if matches!(expr, Expr::ILike { .. })
                || matches!(
                    expr,
                    Expr::BinaryOp {
                        op: BinaryOperator::Spaceship,
                        ..
                    }
                )
            {
                return self.reject("operator is not supported by Trino");
            }
            if let Expr::Function(function) = expr {
                if self
                    .validate_expression_function_name(&function.name)
                    .is_break()
                {
                    return ControlFlow::Break(());
                }
                if function.name.0.len() > 1 {
                    return ControlFlow::Continue(());
                }
                let Some(name) = function.name.0.first().and_then(|part| part.as_ident()) else {
                    return ControlFlow::Continue(());
                };
                let name = name.value.to_ascii_lowercase();
                let expected = match name.as_str() {
                    "if" => Some((2usize, Some(3usize))),
                    "nullif" => Some((2, Some(2))),
                    "coalesce" => Some((2, None)),
                    "try" => Some((1, Some(1))),
                    "format" => Some((2, None)),
                    _ => None,
                };
                if let Some((minimum, maximum)) = expected {
                    let valid_arguments = match &function.args {
                        FunctionArguments::List(arguments) => {
                            let count = arguments.args.len();
                            arguments.duplicate_treatment.is_none()
                                && arguments.clauses.is_empty()
                                && count >= minimum
                                && maximum.map_or(true, |maximum| count <= maximum)
                                && arguments.args.iter().all(|argument| {
                                    matches!(
                                        argument,
                                        FunctionArg::Unnamed(FunctionArgExpr::Expr(_))
                                    ) || (count == 1
                                        && matches!(
                                            argument,
                                            FunctionArg::Unnamed(
                                                FunctionArgExpr::QualifiedWildcard(_)
                                            )
                                        ))
                                })
                        }
                        _ => false,
                    };
                    if !matches!(function.parameters, FunctionArguments::None)
                        || !valid_arguments
                        || function.filter.is_some()
                        || function.null_treatment.is_some()
                        || function.over.is_some()
                        || !function.within_group.is_empty()
                    {
                        return self.reject_at_ident(
                            "invalid Trino parser-special function syntax",
                            function
                                .name
                                .0
                                .first()
                                .and_then(|part| part.as_ident())
                                .expect("single-part function name must be an identifier"),
                        );
                    }
                }
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_relation(
            &mut self,
            relation: &sqlparser::ast::ObjectName,
        ) -> ControlFlow<Self::Break> {
            for ident in relation.0.iter().filter_map(|part| part.as_ident()) {
                if self.validate_ident(ident).is_break() {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_table_factor(
            &mut self,
            table_factor: &TableFactor,
        ) -> ControlFlow<Self::Break> {
            if let TableFactor::MatchRecognize { measures, .. } = table_factor {
                for measure in measures {
                    if self.validate_match_measure_alias(&measure.alias).is_break() {
                        return ControlFlow::Break(());
                    }
                }
            }
            match table_factor {
                TableFactor::Table {
                    sample: Some(sample),
                    ..
                }
                | TableFactor::Derived {
                    sample: Some(sample),
                    ..
                } if !is_trino_table_sample(sample) => {
                    return self.reject("invalid Trino TABLESAMPLE syntax");
                }
                _ => {}
            }
            if matches!(table_factor, TableFactor::Table { args: Some(_), .. })
                || matches!(table_factor, TableFactor::Function { .. })
            {
                return self.reject("named table functions require TABLE(...) in Trino");
            }
            ControlFlow::Continue(())
        }
    }

    let mut visitor = GrammarVisitor {
        error: &mut error,
        source_tokens,
    };
    let _ = statements.to_vec().visit(&mut visitor);
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn normalize_datetime_type_parameters(tokens: &mut [TokenWithSpan]) {
    for data_type in 0..tokens.len() {
        if (!is_unquoted_word(&tokens[data_type], "time")
            && !is_unquoted_word(&tokens[data_type], "timestamp"))
            || !is_structural_type_position(tokens, data_type)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, data_type + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        let Some(parameter) = next_significant(tokens, open + 1, close) else {
            continue;
        };
        if next_significant(tokens, parameter + 1, close).is_none()
            && matches!(tokens[parameter].token, Token::Word(_))
        {
            tokens[parameter].token = Token::Number("0".to_string(), false);
        }
    }
}

fn interval_precision_is_valid(
    tokens: &[TokenWithSpan],
    open: usize,
    close: usize,
    maximum_parameters: usize,
) -> bool {
    let mut cursor = match next_significant(tokens, open + 1, close) {
        Some(cursor) => cursor,
        None => return false,
    };
    let mut parameters = 0usize;
    loop {
        if !matches!(tokens[cursor].token, Token::Number(_, false)) {
            return false;
        }
        parameters += 1;
        let Some(comma) = next_significant(tokens, cursor + 1, close) else {
            return parameters <= maximum_parameters;
        };
        if tokens[comma].token != Token::Comma || parameters >= maximum_parameters {
            return false;
        }
        let Some(parameter) = next_significant(tokens, comma + 1, close) else {
            return false;
        };
        cursor = parameter;
    }
}

fn normalize_interval_type_precision(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for interval in 0..tokens.len() {
        if !is_unquoted_word(&tokens[interval], "interval")
            || !is_structural_type_position(tokens, interval)
        {
            continue;
        }
        let Some(start_unit) = next_significant(tokens, interval + 1, tokens.len()) else {
            continue;
        };
        if !is_interval_unit(&tokens[start_unit]) {
            continue;
        }
        let mut cursor = next_significant(tokens, start_unit + 1, tokens.len());
        if cursor.is_some_and(|open| tokens[open].token == Token::LParen) {
            let open = cursor.unwrap_or(start_unit);
            let close = matching_rparen(tokens, open)
                .ok_or_else(|| syntax_error(&tokens[open], "unterminated interval precision"))?;
            let maximum = if is_unquoted_word(&tokens[start_unit], "second") {
                2
            } else {
                1
            };
            if !interval_precision_is_valid(tokens, open, close, maximum) {
                return Err(syntax_error(&tokens[open], "invalid interval precision"));
            }
            blank_non_whitespace(tokens, open, close + 1);
            cursor = next_significant(tokens, close + 1, tokens.len());
        }
        let Some(to) = cursor else {
            continue;
        };
        if !is_unquoted_word(&tokens[to], "to") {
            continue;
        }
        let Some(end_unit) = next_significant(tokens, to + 1, tokens.len()) else {
            continue;
        };
        let Some(open) = next_significant(tokens, end_unit + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let close = matching_rparen(tokens, open)
            .ok_or_else(|| syntax_error(&tokens[open], "unterminated interval precision"))?;
        if !is_unquoted_word(&tokens[end_unit], "second")
            || !interval_precision_is_valid(tokens, open, close, 1)
        {
            return Err(syntax_error(&tokens[open], "invalid interval precision"));
        }
        blank_non_whitespace(tokens, open, close + 1);
    }
    Ok(())
}

fn normalize_legacy_map_types(tokens: &mut [TokenWithSpan]) -> Result<(), ParserError> {
    for map in 0..tokens.len() {
        if !is_unquoted_word(&tokens[map], "map") || !is_structural_type_position(tokens, map) {
            continue;
        }
        let Some(open) = next_significant(tokens, map + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token == Token::Lt {
            let close = matching_gt(tokens, open)
                .ok_or_else(|| syntax_error(&tokens[open], "unterminated MAP type"))?;
            let mut depth = 0usize;
            let mut comma = None;
            for (index, token) in tokens.iter().enumerate().take(close).skip(open + 1) {
                match token.token {
                    Token::Lt | Token::LParen | Token::LBracket => depth += 1,
                    Token::Gt | Token::RParen | Token::RBracket if depth > 0 => depth -= 1,
                    Token::Comma if depth == 0 => {
                        if comma.is_some() {
                            return Err(syntax_error(
                                token,
                                "MAP type requires exactly two parameters",
                            ));
                        }
                        comma = Some(index);
                    }
                    _ => {}
                }
            }
            let comma = comma.ok_or_else(|| {
                syntax_error(&tokens[open], "MAP type requires exactly two parameters")
            })?;
            if next_significant(tokens, open + 1, comma).is_none()
                || next_significant(tokens, comma + 1, close).is_none()
            {
                return Err(syntax_error(
                    &tokens[comma],
                    "MAP type requires exactly two parameters",
                ));
            }
            replace_word(&mut tokens[map], "STRUCT", Keyword::STRUCT);
        }
    }
    Ok(())
}

fn postfix_array_base(tokens: &[TokenWithSpan], array: usize) -> bool {
    let Some(previous) = previous_significant(tokens, array) else {
        return false;
    };
    match tokens[previous].token {
        Token::Word(_) => is_probable_type_name(&tokens[previous]),
        Token::RParen => matching_lparen(tokens, previous)
            .and_then(|open| previous_significant(tokens, open))
            .is_some_and(|word| is_probable_type_name(&tokens[word])),
        Token::Gt | Token::RBracket => true,
        _ => false,
    }
}

fn postfix_array_base_is_declaration_name(tokens: &[TokenWithSpan], array: usize) -> bool {
    let Some(base) = previous_significant(tokens, array) else {
        return false;
    };
    let Some(before_base) = previous_significant(tokens, base) else {
        return false;
    };
    if !matches!(tokens[before_base].token, Token::LParen | Token::Comma) {
        return false;
    }
    let Some(open) = enclosing_parentheses(tokens, array).last().copied() else {
        return false;
    };
    if is_drop_function_signature_position(tokens, array) {
        return false;
    }
    let container = previous_significant(tokens, open);
    !container.is_some_and(|container| is_unquoted_word(&tokens[container], "row"))
        && is_in_create_type_declaration(tokens, array)
}

fn normalize_postfix_array_types(tokens: &mut Vec<TokenWithSpan>) {
    for array in (0..tokens.len()).rev() {
        let next = next_significant(tokens, array + 1, tokens.len());
        if !is_unquoted_word(&tokens[array], "array")
            || !is_structural_type_position(tokens, array)
            || !postfix_array_base(tokens, array)
            || postfix_array_base_is_declaration_name(tokens, array)
            || next.is_some_and(|next| matches!(tokens[next].token, Token::LParen | Token::Lt))
        {
            continue;
        }
        let source = tokens[array].clone();
        tokens[array] = token_with_span(Token::LBracket, &source);
        tokens.insert(array + 1, token_with_span(Token::RBracket, &source));
    }
}

fn collect_generic_alter_metadata(tokens: &[TokenWithSpan]) -> Vec<Statement> {
    let generic = GenericDialect {};
    let mut metadata = Vec::new();
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let first = next_significant(tokens, start, end);
        let table = first.and_then(|index| next_significant(tokens, index + 1, end));
        if first.is_some_and(|index| is_unquoted_word(&tokens[index], "alter"))
            && table.is_some_and(|index| is_unquoted_word(&tokens[index], "table"))
        {
            let mut statement = tokens[start..end].to_vec();
            let source = tokens
                .get(end)
                .or_else(|| tokens.get(end.saturating_sub(1)))
                .expect("tokenized SQL always contains EOF");
            statement.push(token_with_span(Token::EOF, source));
            if let Ok(mut parsed) = Parser::new(&generic)
                .with_tokens_with_locations(statement)
                .parse_statements()
            {
                metadata.append(&mut parsed);
            }
        }
        start = end.saturating_add(1);
    }
    metadata
}

fn top_level_rarrow(tokens: &[TokenWithSpan], start: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.checked_sub(1)?;
            }
            Token::RArrow if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn collect_custom_expression_metadata(
    tokens: &[TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<Vec<Statement>, ParserError> {
    let mut metadata = Vec::new();
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let kind = custom_statement_kind(tokens, start, end);
        if matches!(
            kind,
            Some(TrinoStatementKind::Catalog | TrinoStatementKind::Branch)
        ) {
            if let Some(with) = find_top_level_word(tokens, start, end, "with") {
                let open = next_significant(tokens, with + 1, end)
                    .ok_or_else(|| syntax_error(&tokens[with], "WITH requires properties"))?;
                if tokens[open].token == Token::LParen {
                    let close = matching_rparen(tokens, open)
                        .ok_or_else(|| syntax_error(&tokens[open], "unterminated properties"))?;
                    metadata.extend(property_expression_metadata(tokens, open, close, dialect)?);
                }
            }
        }
        if let Some(alter) = find_top_level_word(tokens, start, end, "alter") {
            if let Some(properties) = find_top_level_word(tokens, alter, end, "properties") {
                let property_start =
                    next_significant(tokens, properties + 1, end).ok_or_else(|| {
                        syntax_error(&tokens[properties], "SET PROPERTIES requires assignments")
                    })?;
                if tokens[property_start].token != Token::LParen {
                    for (assignment_start, assignment_end) in
                        top_level_ranges(tokens, property_start, end)?
                    {
                        if let Some(statement) = property_assignment_metadata(
                            tokens,
                            assignment_start,
                            assignment_end,
                            dialect,
                        )? {
                            metadata.push(statement);
                        }
                    }
                }
            }
            if let Some(with) = find_top_level_word(tokens, alter, end, "with") {
                let has_add_column = find_top_level_word(tokens, alter, with, "add").is_some()
                    && find_top_level_word(tokens, alter, with, "column").is_some();
                if has_add_column {
                    if let Some(open) = next_significant(tokens, with + 1, end)
                        .filter(|index| tokens[*index].token == Token::LParen)
                    {
                        let close = matching_rparen(tokens, open).ok_or_else(|| {
                            syntax_error(&tokens[open], "unterminated properties")
                        })?;
                        metadata
                            .extend(property_expression_metadata(tokens, open, close, dialect)?);
                    }
                }
            }
            if let Some(execute) = find_top_level_word(tokens, alter, end, "execute") {
                let procedure = next_significant(tokens, execute + 1, end).ok_or_else(|| {
                    syntax_error(&tokens[execute], "EXECUTE requires a procedure")
                })?;
                let after_procedure = consume_qualified_name(tokens, procedure, end)
                    .ok_or_else(|| syntax_error(&tokens[procedure], "invalid procedure name"))?;
                let mut where_start = after_procedure;
                if let Some(open) = next_significant(tokens, after_procedure, end)
                    .filter(|index| tokens[*index].token == Token::LParen)
                {
                    let close = matching_rparen(tokens, open).ok_or_else(|| {
                        syntax_error(&tokens[open], "unterminated EXECUTE arguments")
                    })?;
                    if next_significant(tokens, open + 1, close).is_some() {
                        for (argument_start, argument_end) in
                            top_level_ranges(tokens, open + 1, close)?
                        {
                            let value_start =
                                top_level_rarrow(tokens, argument_start, argument_end)
                                    .and_then(|arrow| {
                                        next_significant(tokens, arrow + 1, argument_end)
                                    })
                                    .unwrap_or(argument_start);
                            metadata.push(parse_expression_metadata(
                                tokens,
                                value_start,
                                argument_end,
                                dialect,
                            )?);
                        }
                    }
                    where_start = close + 1;
                }
                if let Some(where_index) = find_top_level_word(tokens, where_start, end, "where") {
                    let expression =
                        next_significant(tokens, where_index + 1, end).ok_or_else(|| {
                            syntax_error(&tokens[where_index], "WHERE requires an expression")
                        })?;
                    metadata.push(parse_expression_metadata(tokens, expression, end, dialect)?);
                }
            }
        }
        start = end.saturating_add(1);
    }
    Ok(metadata)
}

fn rewrite_type_container(
    tokens: &mut [TokenWithSpan],
    word_index: usize,
    open: usize,
    close: usize,
) {
    if matches!(
        unquoted_keyword(&tokens[word_index]),
        Some(Keyword::ROW | Keyword::MAP)
    ) {
        let Token::Word(word) = &mut tokens[word_index].token else {
            unreachable!();
        };
        word.value = "STRUCT".to_string();
        word.keyword = Keyword::STRUCT;
    }
    tokens[open].token = Token::Lt;
    tokens[close].token = Token::Gt;

    let mut index = open + 1;
    while index < close {
        if is_type_container(&tokens[index]) {
            if let Some(nested_open) = next_significant(tokens, index + 1, close) {
                if tokens[nested_open].token == Token::LParen {
                    if let Some(nested_close) = matching_rparen(tokens, nested_open) {
                        rewrite_type_container(tokens, index, nested_open, nested_close);
                        index = nested_close + 1;
                        continue;
                    }
                }
            }
        }
        index += 1;
    }
}

fn normalize_nested_row_types(tokens: &mut [TokenWithSpan]) {
    let mut roots = Vec::new();
    for word_index in 0..tokens.len() {
        if unquoted_keyword(&tokens[word_index]) != Some(Keyword::ROW) {
            continue;
        }
        let Some(open) = next_significant(tokens, word_index + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        if !row_group_looks_like_type(tokens, open, close) {
            continue;
        }

        let mut root = (word_index, open, close);
        for parent_open in enclosing_parentheses(tokens, word_index).into_iter().rev() {
            let Some(parent_word) = previous_significant(tokens, parent_open) else {
                break;
            };
            if !is_type_container(&tokens[parent_word]) {
                break;
            }
            let Some(parent_close) = matching_rparen(tokens, parent_open) else {
                break;
            };
            root = (parent_word, parent_open, parent_close);
        }
        if is_type_context(tokens, root.0) {
            roots.push(root);
        }
    }

    roots.sort_unstable_by_key(|root| (root.0, Reverse(root.2)));
    let mut outermost = Vec::new();
    for root in roots {
        if outermost
            .iter()
            .any(|existing: &(usize, usize, usize)| existing.0 <= root.0 && existing.2 >= root.2)
        {
            continue;
        }
        outermost.push(root);
    }
    for (word_index, open, close) in outermost {
        rewrite_type_container(tokens, word_index, open, close);
    }
}

pub(crate) fn parse_sql(dialect: &dyn Dialect, sql: &str) -> Result<ParsedSql, ParserError> {
    let mut tokens = Tokenizer::new(dialect, sql).tokenize_with_location()?;
    let source_tokens = tokens.clone();
    let custom_statement_kinds = collect_custom_statement_kinds(&tokens);
    let source_type_names = collect_source_type_names(&tokens);
    validate_trino_lexical_tokens(&tokens)?;
    validate_empty_statement_segments(&tokens)?;
    validate_balanced_groups(&tokens)?;
    validate_trino_source_shapes(&tokens)?;
    validate_trino_typed_literals(&tokens)?;
    validate_trino_table_samples(&tokens)?;
    validate_trino_create_table_forms(&tokens)?;
    validate_trino_grouping_elements(&tokens)?;
    validate_trino_reserved_expression_starts(&tokens)?;
    validate_trino_count_distinct_wildcard(&tokens)?;
    let mut compatibility_metadata = normalize_create_table_extensions(&mut tokens, dialect)?;
    compatibility_metadata.extend(normalize_analyze_properties(&mut tokens, dialect)?);
    normalize_create_view_options(&mut tokens)?;
    normalize_json_table_scalar_columns(&mut tokens)?;
    normalize_json_object_pairs(&mut tokens)?;
    normalize_json_query_wrappers(&mut tokens)?;
    compatibility_metadata.extend(normalize_json_behaviors(&mut tokens, dialect)?);
    normalize_prepare_from(&mut tokens);
    normalize_nested_row_types(&mut tokens);
    normalize_array_parenthesis_types(&mut tokens);
    normalize_table_queries(&mut tokens);
    normalize_top_identifiers(&mut tokens);
    normalize_non_reserved_projection_words(&mut tokens);
    normalize_contextual_alias_words(&mut tokens);
    normalize_corresponding_set_operations(&mut tokens)?;
    normalize_group_by_quantifiers(&mut tokens)?;
    normalize_group_by_auto(&mut tokens);
    compatibility_metadata.extend(normalize_pivot_group_by(&mut tokens, dialect)?);
    normalize_nearest_relations(&mut tokens, dialect)?;
    normalize_ipaddress_literals(&mut tokens);
    normalize_at_syntax(&mut tokens);
    normalize_iceberg_time_travel(&mut tokens);
    normalize_trino_non_decimal_integer_literals(&mut tokens);
    normalize_between_symmetry(&mut tokens);
    normalize_trim_without_character(&mut tokens);
    normalize_empty_lambdas(&mut tokens);
    normalize_method_calls(&mut tokens);
    normalize_pattern_processing_modes(&mut tokens);
    normalize_open_pattern_quantifiers(&mut tokens);
    normalize_over_projection_alias(&mut tokens);
    normalize_unicode_escapes(&mut tokens)?;
    normalize_scalar_values(&mut tokens);
    normalize_typed_values(&mut tokens);
    normalize_values_expression(&mut tokens, None);
    normalize_values_expression(&mut tokens, Some("map_from_entries"));
    compatibility_metadata.extend(normalize_quantified_values(&mut tokens, dialect)?);
    normalize_scalar_values_relations(&mut tokens, dialect)?;
    normalize_datetime_type_parameters(&mut tokens);
    normalize_interval_type_precision(&mut tokens)?;
    normalize_legacy_map_types(&mut tokens)?;
    normalize_postfix_array_types(&mut tokens);
    validate_trino_identifiers(&tokens)?;
    normalize_iceberg_branch_references(&mut tokens, dialect);
    if let Some(token) = tokens.iter().find(|token| token.token == Token::AtSign) {
        return Err(ParserError::ParserError(format!(
            "unexpected @ outside an Iceberg DML branch reference at Line: {}, Column: {}",
            token.span.start.line, token.span.start.column
        )));
    }
    compatibility_metadata.extend(collect_generic_alter_metadata(&tokens));
    compatibility_metadata.extend(collect_custom_expression_metadata(&tokens, dialect)?);
    let parsed = Parser::new(dialect)
        .with_tokens_with_locations(tokens.clone())
        .parse_statements();
    if let Ok(statements) = parsed {
        validate_trino_aliases(&statements, &source_tokens)?;
        validate_trino_ast(&statements, &source_tokens)?;
        return Ok(ParsedSql {
            statements,
            inline_functions: Vec::new(),
            compatibility_metadata,
            custom_statement_kinds,
            source_type_names,
        });
    }
    normalize_table_function_copartition(&mut tokens)?;
    compatibility_metadata.extend(normalize_table_function_arguments(&mut tokens, dialect)?);
    normalize_materialized_view_options(&mut tokens);
    normalize_match_recognize_subsets(&mut tokens);
    compatibility_metadata.extend(normalize_with_session(&mut tokens, dialect)?);
    let inline_functions = normalize_inline_functions(&mut tokens, dialect)?;
    let row_expansions = normalize_row_expansions(&mut tokens, dialect)?;
    compatibility_metadata.extend(row_expansions);
    let statements = Parser::new(dialect)
        .with_tokens_with_locations(tokens)
        .parse_statements()?;
    validate_trino_aliases(&statements, &source_tokens)?;
    for (_, statement) in &inline_functions {
        validate_trino_aliases(std::slice::from_ref(statement), &source_tokens)?;
    }
    validate_trino_ast(&statements, &source_tokens)?;
    Ok(ParsedSql {
        statements,
        inline_functions,
        compatibility_metadata,
        custom_statement_kinds,
        source_type_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialects::TrinoDialect;

    #[test]
    fn compatibility_normalizers_ignore_literals_and_comments() {
        let sql = "SELECT 'PREPARE p FROM; CAST(x AS ARRAY(BIGINT)); IPADDRESS ''10.0.0.1''; FROM t FOR VERSION AS OF ''audit''; VALUES VARCHAR ''value''; VALUES ARRAY[1]; VALUES map_from_entries(ARRAY[(''x'', 1)])' /* PREPARE p FROM IPADDRESS '10.0.0.1' FOR VERSION AS OF 'audit' */";
        let dialect = TrinoDialect {};
        let mut tokens = Tokenizer::new(&dialect, sql)
            .tokenize_with_location()
            .unwrap();
        let original = tokens.clone();

        normalize_prepare_from(&mut tokens);
        normalize_array_parenthesis_types(&mut tokens);
        normalize_top_identifiers(&mut tokens);
        normalize_ipaddress_literals(&mut tokens);
        normalize_iceberg_time_travel(&mut tokens);
        normalize_scalar_values(&mut tokens);
        normalize_typed_values(&mut tokens);
        normalize_values_expression(&mut tokens, None);
        normalize_values_expression(&mut tokens, Some("map_from_entries"));

        assert_eq!(tokens, original);
    }

    #[test]
    fn iceberg_time_travel_is_limited_to_relations() {
        let dialect = TrinoDialect {};
        assert!(parse_sql(&dialect, "SELECT * FROM customer FOR VERSION AS OF 'audit'").is_ok());
        assert!(parse_sql(&dialect, "SELECT customer FOR VERSION AS OF 1").is_err());
    }

    #[test]
    fn inline_function_declaration_uses_create_function_parser() {
        let dialect = TrinoDialect {};
        let sql = "WITH FUNCTION hello(name VARCHAR) RETURNS VARCHAR RETURN format('Hello %s!', name) SELECT hello('Finn')";
        let parsed = parse_sql(&dialect, sql).unwrap();

        assert_eq!(parsed.statements.len(), 1);
        assert_eq!(parsed.inline_functions.len(), 1);
        assert!(parsed.compatibility_metadata.is_empty());
        assert!(matches!(
            parsed.inline_functions[0].1,
            Statement::CreateFunction(_)
        ));
    }

    #[test]
    fn with_session_values_are_preserved_as_metadata() {
        let dialect = TrinoDialect {};
        let parsed = parse_sql(
            &dialect,
            "WITH SESSION example.setting = marh(CAST(1 AS bignum)) SELECT 1",
        )
        .unwrap();

        assert_eq!(parsed.compatibility_metadata.len(), 1);
    }

    #[test]
    fn inline_compound_function_uses_structural_routine_parser() {
        let dialect = TrinoDialect {};
        let parsed = parse_sql(
            &dialect,
            "WITH FUNCTION local_f(value bigint) RETURNS bigint BEGIN DECLARE result bigint DEFAULT 1; RETURN result; END SELECT local_f(1)",
        )
        .unwrap();

        assert_eq!(parsed.inline_functions.len(), 1);
        assert_eq!(parsed.statements.len(), 1);
    }

    #[test]
    fn row_expansion_preserves_field_expressions_as_metadata() {
        let dialect = TrinoDialect {};
        let parsed = parse_sql(
            &dialect,
            "SELECT ROW(1 AS first, marh() second).* AS (left_value, right_value)",
        )
        .unwrap();

        assert_eq!(parsed.statements.len(), 1);
        assert!(parsed.inline_functions.is_empty());
        assert_eq!(parsed.compatibility_metadata.len(), 2);
        assert!(parse_sql(&dialect, "SELECT ROW().*").is_err());
    }

    #[test]
    fn group_by_quantifiers_require_a_grouping_element() {
        let dialect = TrinoDialect {};
        assert!(parse_sql(
            &dialect,
            "SELECT a, b, sum(c) FROM t GROUP BY DISTINCT ROLLUP ((a, b), c)",
        )
        .is_ok());
        assert!(parse_sql(&dialect, "SELECT a, sum(b) FROM t GROUP BY ALL").is_err());

        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY ROLLUP ()").is_err());
        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY CUBE ()").is_err());
        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY ROLLUP (())").is_ok());
        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY CUBE (())").is_ok());
        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY GROUPING SETS ()").is_err());
        assert!(parse_sql(&dialect, "SELECT rollup()").is_err());
        assert!(parse_sql(&dialect, "SELECT cube()").is_err());
        assert!(parse_sql(&dialect, "SELECT \"rollup\"()").is_ok());
    }

    #[test]
    fn at_local_distinguishes_the_operator_from_identifiers() {
        let dialect = TrinoDialect {};
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT LOCAL").is_ok());
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT").is_ok());
        assert!(parse_sql(&dialect, "SELECT At LOCAL FROM t").is_ok());
        assert!(parse_sql(&dialect, "SELECT a + At LOCAL FROM t").is_ok());
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT TIME").is_err());
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT UTC").is_err());
    }

    #[test]
    fn scalar_values_relations_require_expressions() {
        let dialect = TrinoDialect {};
        assert!(parse_sql(&dialect, "SELECT * FROM LATERAL (VALUES 1, 2)").is_ok());
        assert!(parse_sql(&dialect, "SELECT * FROM LATERAL (VALUES )").is_err());
        assert!(parse_sql(&dialect, "INSERT INTO target VALUES 1").is_ok());
        assert!(parse_sql(&dialect, "CREATE TABLE foo () AS (VALUES 1)").is_err());
        assert!(parse_sql(&dialect, "SELECT count(DISTINCT *) FROM (VALUES 1)").is_err());
    }

    #[test]
    fn current_statement_extensions_are_structurally_normalized() {
        let dialect = TrinoDialect {};
        for sql in [
            "CREATE TABLE t (c VARCHAR WITH (compression = 'LZ4'))",
            "CREATE TABLE t (items ARRAY(VARCHAR)) COMMENT 'items' WITH (format = 'PARQUET')",
            "CREATE TABLE t (LIKE source INCLUDING PROPERTIES)",
            "CREATE TABLE t(x, y) AS SELECT a, b FROM source WITH NO DATA",
            "CREATE TABLE t COMMENT 'copy' WITH (format = 'PARQUET') AS SELECT 1 WITH DATA",
            "ANALYZE t WITH (sample = 10)",
            "CREATE VIEW v COMMENT 'v' SECURITY DEFINER AS SELECT 1",
            "SELECT ALL, SOME, ANY FROM t",
            "TABLE nation ORDER BY nationkey LIMIT 3",
            "SELECT JSON_VALUE(payload, 'lax $.n' RETURNING integer DEFAULT 0 ON EMPTY) FROM events",
            "SELECT JSON_QUERY(payload, 'lax $' WITH CONDITIONAL ARRAY WRAPPER) FROM events",
            "SELECT JSON_QUERY(payload, 'lax $' OMIT QUOTES NULL ON ERROR) FROM events",
            "SELECT JSON_QUERY(payload, 'lax $' KEEP QUOTES) FROM events",
            "SELECT JSON_QUERY(payload, 'lax $' OMIT QUOTES ON SCALAR STRING) FROM events",
            "SELECT JSON_QUERY(payload, 'lax $' WITH UNCONDITIONAL WRAPPER KEEP QUOTES) FROM events",
            "SELECT JSON_OBJECT('k' VALUE 1, KEY 'name' VALUE 'x'), JSON_OBJECT(KEY 'a' VALUE NULL NULL ON NULL)",
            "SELECT JSON_OBJECT('k' : 1, 'name' : 'x')",
            "SELECT * FROM JSON_TABLE(col, 'lax $' COLUMNS(name varchar FORMAT JSON PATH 'lax $.name' WITH WRAPPER KEEP QUOTES NULL ON EMPTY, regions varchar FORMAT JSON ENCODING UTF16 PATH 'lax $.regions' EMPTY ARRAY ON EMPTY EMPTY OBJECT ON ERROR) EMPTY ON ERROR)",
        ] {
            if let Err(error) = parse_sql(&dialect, sql) {
                panic!("expected to parse: {sql}: {error}");
            }
        }
    }

    #[test]
    fn column_time_zone_types_survive_multi_statement_fallback() {
        let dialect = TrinoDialect {};
        let sql = "
            CREATE TABLE events (
                event_time time(6) with time zone,
                event_ts timestamp(6) with time zone
            );
            SELECT * FROM TABLE(exclude_columns(
                input => TABLE(events),
                columns => DESCRIPTOR(event_time)
            ))
        ";

        let parsed = parse_sql(&dialect, sql).expect("expected to parse");

        assert_eq!(parsed.statements.len(), 2);
    }

    #[test]
    fn malformed_statement_extensions_are_rejected() {
        let dialect = TrinoDialect {};
        for sql in [
            "CREATE TABLE t (c VARCHAR WITH ())",
            "CREATE TABLE t (items ARRAY(VARCHAR)) COMMENT 1 WITH (format = 'PARQUET')",
            "CREATE TABLE t COMMENT 'one' COMMENT 'two' AS SELECT 1",
            "CREATE TABLE t COMMENT 'one' WITH (format = 'ORC') WITH (x = 1) AS SELECT 1",
            "CREATE TABLE t COMMENT 'one' WITH MAYBE DATA",
            "CREATE TABLE t COMMENT 'one' AS",
            "CREATE TABLE t (LIKE source INCLUDING)",
            "CREATE OR REPLACE TABLE IF NOT EXISTS t AS SELECT 1",
            "ANALYZE t WITH (sample =)",
            "CREATE VIEW v SECURITY OWNER AS SELECT 1",
            "SELECT * FROM JSON_TABLE(col, '$' COLUMNS(name varchar FORMAT XML PATH '$'))",
            "SELECT JSON_VALUE(payload, '$.x' DEFAULT ON EMPTY)",
            "SELECT JSON_QUERY(payload, '$' WITH CONDITIONAL)",
            "SELECT JSON_QUERY(payload, '$' KEEP QUOTES ON SCALAR)",
            "SELECT JSON_QUERY(payload, '$' KEEP QUOTES OMIT QUOTES)",
            "SELECT JSON_OBJECT('k' VALUE)",
            "TABLE 1",
            "TABLE nation alias",
        ] {
            assert!(
                parse_sql(&dialect, sql).is_err(),
                "expected to reject: {sql}"
            );
        }
    }

    #[test]
    fn v018_rejects_previous_false_accepts() {
        let dialect = TrinoDialect {};
        for sql in [
            "SELECT orderkey, custkey, FROM orders",
            "SELECT orderkey, custkey, totalprice, FROM orders WHERE orderkey = 1",
            "SELECT * FROM orders o JOIN customers c",
            "SELECT X'ABC' FROM orders",
            "SELECT * FROM select",
            "CREATE TABLE where (id integer)",
            "SELECT from FROM orders",
            "CREATE TABLE group (id integer, order integer)",
            "WITH big (SELECT * FROM orders) SELECT * FROM big",
            "SELECT * FROM orders o JOIN customers c USING (o.custkey)",
            "SELECT IF(totalprice > 100) FROM orders",
            "SELECT coalesce() FROM orders",
            "SELECT nullif(orderstatus, 'O', 'F') FROM orders",
            "INSERT orders VALUES (1)",
            "DELETE orders WHERE orderkey = 1",
            "TRUNCATE orders",
            "SELECT * FROM orders FETCH FIRST 5",
            "SELECT * FROM orders FETCH NEXT 5 ONLY",
            "SELECT * FROM orders LIMIT 10 FETCH FIRST 5 ROWS ONLY",
            "SELECT JSON_VALUE(payload) FROM events",
            "SELECT json_extract(payload, $.page) FROM events",
            "SELECT * FROM orders MATCH_RECOGNIZE (MEASURES match_number() mn PATTERN (a) DEFINE a AS true)",
            "EXECUTE USING 1",
            "DEALLOCATE stmt1",
            "SELECT * FROM orders;;",
            "SELECT * FROM orders;;;",
            ";",
            "SELECT * FROM orders WHERE orderkey == 1",
            "SELECT INTERVAL '3' TO DAY FROM orders",
            "SELECT * FROM orders WITH ORDINALITY",
            "EXPLAIN (TYPE UNKNOWN) SELECT * FROM orders",
            "SHOW",
            "SELECT order-key FROM orders",
            "SELECT CURRENT_DATE() FROM orders",
            "SELECT CURRENT_TIMESTAMP(3, 4) FROM orders",
            "SELECT schema.json_value(payload), schema.current_timestamp(3, 4) FROM events",
        ] {
            assert!(
                parse_sql(&dialect, sql).is_err(),
                "expected to reject: {sql}"
            );
        }
    }

    #[test]
    fn v018_preserves_positive_neighbors() {
        let dialect = TrinoDialect {};
        for sql in [
            "SELECT orderkey, custkey",
            "SELECT 'closed\nstring' FROM orders",
            "SELECT row_number() OVER FROM orders",
            "SELECT sum(totalprice) OVER (ORDER BY orderdate ROWS 2 PRECEDING) FROM orders",
            "INSERT INTO orders VALUES 1, 'O', 100",
            "SELECT * FROM orders FETCH FIRST ROWS ONLY",
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (a{,}) DEFINE a AS true)",
            "SELECT * FROM orders MATCH_RECOGNIZE (PATTERN (a{,}?) DEFINE a AS true)",
            "SELECT INTERVAL '3' FROM orders",
            "SELECT ROW(1, 2) FROM orders",
            "SELECT count(orderkey) FROM orders",
            "SELECT X'AB CD' FROM orders",
            "SELECT * FROM orders OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY",
            "SELECT if(true, 1), if(false, 1, 2), nullif(1, 2), coalesce(1, 2), try(1 / 0), format('%s', 1)",
            "SELECT try(t.*) FROM t",
            "SELECT t::values()",
            "SELECT \"schema\".\"json_value\"(payload), \"schema\".\"current_timestamp\"(3, 4) FROM events",
            "EXECUTE statement_name USING 1",
            "EXECUTE IMMEDIATE 'SELECT ?' USING 1",
            "SELECT CURRENT_DATE, CURRENT_TIME(3), CURRENT_TIMESTAMP, LOCALTIME(2), LOCALTIMESTAMP",
            "SELECT mktsegment, sum(acctbal) FROM shipping GROUP BY AUTO",
            "SELECT 1 AS FETCH, 2 AS ORDINALITY",
            "SELECT \"from\".\"where\" FROM \"select\" AS \"from\"",
            "SELECT 1; SELECT 2;",
            "CREATE FUNCTION f(x BIGINT) RETURNS BIGINT BEGIN RETURN x + 1; END;",
        ] {
            if let Err(error) = parse_sql(&dialect, sql) {
                panic!("expected to parse: {sql}: {error}");
            }
        }
        assert!(parse_sql(&dialect, "SELECT pattern(a{,})").is_err());
    }

    #[test]
    fn v018_post_review_rejects_structural_gaps() {
        let dialect = TrinoDialect {};
        for sql in [
            "WITH a AS (SELECT 1), b (SELECT 2) SELECT * FROM b",
            "WITH a AS (SELECT 1), b(x) (SELECT 2) SELECT * FROM b",
            "WITH a AS (SELECT 1), b AS (SELECT 2), c (SELECT 3) SELECT * FROM c",
            "WITH a AS (WITH b (SELECT 1) SELECT * FROM b) SELECT * FROM a",
            "WITH x(\"select\", \"where\") (SELECT 1, 2) SELECT * FROM x",
            "SELECT RUNNING if(true, 1)",
            "SELECT RUNNING \"if\"(true, 1)",
            "SELECT FINAL coalesce(1, 2)",
            "SELECT if(t.*, 1) FROM t",
            "SELECT nullif(t.*, 1) FROM t",
            "SELECT coalesce(t.*, 1) FROM t",
            "SELECT try(t.*, 1) FROM t",
            "SELECT format(t.*, 1) FROM t",
            "CREATE VIEW select AS SELECT 1",
            "DROP VIEW select",
            "CREATE SCHEMA select",
            "ALTER TABLE select RENAME TO x",
            "GRANT SELECT ON TABLE select TO ROLE analyst",
            "PREPARE select FROM SELECT 1",
            "EXECUTE select",
            "DEALLOCATE PREPARE select",
            "CREATE FUNCTION f(select BIGINT) RETURNS BIGINT RETURN 1",
            "CREATE FUNCTION f(x ROW(select BIGINT)) RETURNS BIGINT RETURN 1",
            "SELECT CAST(ROW(1) AS ROW(select BIGINT))",
            "SELECT * FROM a JOIN b USING (select)",
            "SELECT where(1)",
            "CALL select()",
        ] {
            assert!(
                parse_sql(&dialect, sql).is_err(),
                "expected to reject: {sql}"
            );
        }
    }

    #[test]
    fn v018_post_review_preserves_quoted_and_non_reserved_neighbors() {
        let dialect = TrinoDialect {};
        for sql in [
            "WITH a AS (SELECT 1), b AS (SELECT 2), c(x) AS (SELECT 3) SELECT * FROM c",
            "WITH outer_cte AS (WITH inner_cte AS (SELECT 1) SELECT * FROM inner_cte) SELECT * FROM outer_cte",
            "WITH x(\"select\", \"where\") AS (SELECT 1, 2) SELECT * FROM x",
            "SELECT RUNNING sum(x) FROM t",
            "SELECT FINAL first(x) FROM t",
            "SELECT try(t.*) FROM t",
            "CREATE VIEW \"select\" AS SELECT 1",
            "DROP VIEW \"select\"",
            "CREATE SCHEMA \"select\"",
            "ALTER TABLE \"select\" RENAME TO x",
            "GRANT SELECT ON TABLE \"select\" TO ROLE analyst",
            "PREPARE \"select\" FROM SELECT 1",
            "EXECUTE \"select\"",
            "DEALLOCATE PREPARE \"select\"",
            "CREATE FUNCTION \"select\"(\"where\" BIGINT) RETURNS BIGINT RETURN \"where\"",
            "CREATE FUNCTION f(x ROW(\"select\" BIGINT)) RETURNS ROW(\"where\" BIGINT) RETURN CAST(ROW(1) AS ROW(\"where\" BIGINT))",
            "SELECT CAST(ROW(1) AS ROW(\"select\" BIGINT))",
            "SELECT * FROM a JOIN b USING (\"select\")",
            "SELECT \"where\"(1)",
            "CALL \"select\"()",
            "SELECT * FROM t MATCH_RECOGNIZE (MEASURES after + 1 AS first_value, A.x AS second_value PATTERN (A) DEFINE A AS true)",
        ] {
            if let Err(error) = parse_sql(&dialect, sql) {
                panic!("expected to parse: {sql}: {error}");
            }
        }

        for word in ["after", "all", "one", "pattern", "subset", "define"] {
            let sql = format!(
                "SELECT * FROM t MATCH_RECOGNIZE (MEASURES {word} + 1 AS m PATTERN (A) DEFINE A AS true)"
            );
            if let Err(error) = parse_sql(&dialect, &sql) {
                panic!("expected to parse: {sql}: {error}");
            }
        }
    }
}
