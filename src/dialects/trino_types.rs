use std::cmp::Reverse;

use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer, Whitespace, Word};

use crate::types;

pub(crate) struct ParsedSql {
    pub(crate) statements: Vec<Statement>,
    pub(crate) inline_functions: Vec<Statement>,
    pub(crate) compatibility_metadata: Vec<Statement>,
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

fn syntax_error(token: &TokenWithSpan, message: &str) -> ParserError {
    ParserError::ParserError(format!(
        "{message} at Line: {}, Column: {}",
        token.span.start.line, token.span.start.column
    ))
}

fn validate_balanced_groups(tokens: &[TokenWithSpan]) -> Result<(), ParserError> {
    let mut groups = Vec::new();
    for token in tokens {
        match token.token {
            Token::LParen => groups.push((token, Token::RParen)),
            Token::LBracket => groups.push((token, Token::RBracket)),
            Token::LBrace => groups.push((token, Token::RBrace)),
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
        let Some(method) = next_significant(tokens, sample + 1, tokens.len()) else {
            return Err(syntax_error(
                &tokens[sample],
                "TABLESAMPLE requires a method",
            ));
        };
        if !is_unquoted_word(&tokens[method], "bernoulli")
            && !is_unquoted_word(&tokens[method], "system")
        {
            return Err(syntax_error(
                &tokens[method],
                "TABLESAMPLE requires BERNOULLI or SYSTEM",
            ));
        }
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
        if cursor.is_some_and(|index| is_unquoted_word(&tokens[index], "or")) {
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
        let valid_definition = definition.is_some_and(|index| {
            if tokens[index].token == Token::LParen {
                return matching_rparen(tokens, index)
                    .is_some_and(|close| next_significant(tokens, index + 1, close).is_some());
            }
            if is_unquoted_word(&tokens[index], "as") || is_unquoted_word(&tokens[index], "like") {
                return true;
            }
            if !is_unquoted_word(&tokens[index], "with") {
                return false;
            }
            let Some(open) = next_significant(tokens, index + 1, end) else {
                return false;
            };
            let Some(close) = matching_rparen(tokens, open) else {
                return false;
            };
            next_significant(tokens, close + 1, end)
                .is_some_and(|next| is_unquoted_word(&tokens[next], "as"))
        });
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
        if !is_unquoted_word(&tokens[array], "array")
            || !previous_significant(tokens, array)
                .is_some_and(|previous| is_unquoted_word(&tokens[previous], "as"))
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

fn normalize_empty_grouping_elements(tokens: &mut Vec<TokenWithSpan>) {
    for grouping in (0..tokens.len()).rev() {
        if (!is_unquoted_word(&tokens[grouping], "rollup")
            && !is_unquoted_word(&tokens[grouping], "cube"))
            || !is_grouping_element_position(tokens, grouping)
        {
            continue;
        }
        let Some(open) = next_significant(tokens, grouping + 1, tokens.len()) else {
            continue;
        };
        if tokens[open].token != Token::LParen {
            continue;
        }
        let Some(close) = matching_rparen(tokens, open) else {
            continue;
        };
        if next_significant(tokens, open + 1, close).is_some() {
            continue;
        }
        tokens.insert(
            open + 1,
            word_with_span(&tokens[grouping], "NULL", Keyword::NULL),
        );
    }
}

fn normalize_pivot_group_by(
    tokens: &mut [TokenWithSpan],
    dialect: &dyn Dialect,
) -> Result<(), ParserError> {
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
        blank_non_whitespace(tokens, group, close);
    }
    Ok(())
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
    expression.push(token_with_span(Token::EOF, &tokens[end]));
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

fn normalize_at_local(tokens: &mut Vec<TokenWithSpan>) -> Result<(), ParserError> {
    for at in (0..tokens.len()).rev() {
        if !is_unquoted_word(&tokens[at], "at") {
            continue;
        }
        let Some(modifier) = next_significant(tokens, at + 1, tokens.len()) else {
            return Err(syntax_error(&tokens[at], "AT requires TIME ZONE or LOCAL"));
        };
        if is_unquoted_word(&tokens[modifier], "local") {
            let source = tokens[modifier].clone();
            replace_word(&mut tokens[modifier], "TIME", Keyword::TIME);
            tokens.insert(modifier + 1, word_with_span(&source, "ZONE", Keyword::ZONE));
            tokens.insert(
                modifier + 2,
                token_with_span(Token::SingleQuotedString("UTC".to_string()), &source),
            );
            continue;
        }
        if !is_unquoted_word(&tokens[modifier], "time") {
            return Err(syntax_error(
                &tokens[modifier],
                "AT requires TIME ZONE or LOCAL",
            ));
        }
        let Some(zone) = next_significant(tokens, modifier + 1, tokens.len()) else {
            return Err(syntax_error(&tokens[modifier], "AT TIME requires ZONE"));
        };
        if !is_unquoted_word(&tokens[zone], "zone") {
            return Err(syntax_error(&tokens[zone], "AT TIME requires ZONE"));
        }
    }
    Ok(())
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
        let Some(value) = next_significant(tokens, of + 1, tokens.len()) else {
            continue;
        };
        if !is_unquoted_word(&tokens[as_index], "as") || !is_unquoted_word(&tokens[of], "of") {
            continue;
        }
        if is_unquoted_word(&tokens[kind], "timestamp") {
            tokens[for_index].token = Token::Whitespace(Whitespace::Space);
        } else if is_unquoted_word(&tokens[kind], "version")
            && matches!(
                tokens[value].token,
                Token::Number(_, _) | Token::SingleQuotedString(_)
            )
        {
            tokens[for_index].token = Token::Whitespace(Whitespace::Space);
            if matches!(tokens[value].token, Token::SingleQuotedString(_)) {
                tokens[value].token = Token::Number("0".to_string(), false);
            }
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
    if next_significant(tokens, statement_start(tokens, values), values) == Some(values) {
        return true;
    }
    let Some(open) = enclosing_parentheses(tokens, values).last().copied() else {
        return false;
    };
    previous_significant(tokens, open).is_some_and(|previous| {
        ["from", "join", "lateral", "as"]
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

fn inline_function_return(tokens: &[TokenWithSpan], start: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(start + 1) {
        if matches!(token.token, Token::Whitespace(_)) {
            continue;
        }
        if depth == 0 && is_unquoted_word(token, "return") {
            return Some(index);
        }
        match token.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

fn inline_function_boundary(
    tokens: &[TokenWithSpan],
    function: usize,
    end: usize,
) -> Result<usize, ParserError> {
    let Some(return_index) = inline_function_return(tokens, function, end) else {
        return Err(syntax_error(
            &tokens[function],
            "WITH FUNCTION is missing its RETURN expression",
        ));
    };
    let Some(body) = next_significant(tokens, return_index + 1, end) else {
        return Err(syntax_error(
            &tokens[return_index],
            "WITH FUNCTION is missing its RETURN expression",
        ));
    };
    if !matches!(tokens[body].token, Token::LParen) && is_root_query_start(&tokens[body]) {
        return Err(syntax_error(
            &tokens[body],
            "WITH FUNCTION is missing its RETURN expression",
        ));
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(body) {
        if matches!(token.token, Token::Whitespace(_)) {
            continue;
        }
        if depth == 0
            && index != body
            && (token.token == Token::Comma || is_inline_function_query_start(tokens, index, end))
        {
            return Ok(index);
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
    Err(syntax_error(
        &tokens[function],
        "WITH FUNCTION requires a following query",
    ))
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
) -> Result<Vec<Statement>, ParserError> {
    let mut declarations = Vec::new();
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        let Some(with) = next_significant(tokens, start, end) else {
            break;
        };
        if !is_unquoted_word(&tokens[with], "with") {
            start = end.saturating_add(1);
            continue;
        }
        let Some(mut function) = next_significant(tokens, with + 1, end) else {
            start = end.saturating_add(1);
            continue;
        };
        if !is_unquoted_word(&tokens[function], "function") {
            start = end.saturating_add(1);
            continue;
        }

        loop {
            let boundary = inline_function_boundary(tokens, function, end)?;
            declarations.push(parse_inline_function(tokens, function, boundary, dialect)?);
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
            break;
        }
        start = end.saturating_add(1);
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
        if depth == 0
            && value
            && (matches!(token, Token::Comma) || is_root_query_start(&tokens[index]))
        {
            return Some(index);
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
) -> Option<usize> {
    let with = next_significant(tokens, start, end)?;
    if !is_unquoted_word(&tokens[with], "with") {
        return None;
    }
    let session = next_significant(tokens, with + 1, end)?;
    if !is_unquoted_word(&tokens[session], "session") {
        return None;
    }
    let mut property = next_significant(tokens, session + 1, end)?;
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
        if tokens[value_end].token == Token::Comma {
            property = next_significant(tokens, value_end + 1, end)?;
            continue;
        }
        return is_root_query_start(&tokens[value_end]).then_some(value_end);
    }
}

fn normalize_with_session(tokens: &mut [TokenWithSpan], dialect: &dyn Dialect) {
    let mut start = 0usize;
    while start < tokens.len() {
        let end = statement_end(tokens, start);
        if let Some(query_start) = with_session_query_start(tokens, start, end, dialect) {
            blank_non_whitespace(tokens, start, query_start);
        }
        start = end.saturating_add(1);
    }
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
    let Some(first) = next_significant(tokens, start, index) else {
        return false;
    };
    if unquoted_keyword(&tokens[first]) != Some(Keyword::CREATE) {
        return false;
    }

    let mut cursor = first + 1;
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
    validate_balanced_groups(&tokens)?;
    validate_trino_typed_literals(&tokens)?;
    validate_trino_table_samples(&tokens)?;
    validate_trino_create_table_forms(&tokens)?;
    validate_trino_reserved_expression_starts(&tokens)?;
    validate_trino_count_distinct_wildcard(&tokens)?;
    normalize_prepare_from(&mut tokens);
    normalize_array_parenthesis_types(&mut tokens);
    normalize_top_identifiers(&mut tokens);
    normalize_corresponding_set_operations(&mut tokens)?;
    normalize_group_by_quantifiers(&mut tokens)?;
    normalize_empty_grouping_elements(&mut tokens);
    normalize_pivot_group_by(&mut tokens, dialect)?;
    normalize_nearest_relations(&mut tokens, dialect)?;
    normalize_ipaddress_literals(&mut tokens);
    normalize_at_local(&mut tokens)?;
    normalize_iceberg_time_travel(&mut tokens);
    normalize_scalar_values(&mut tokens);
    normalize_typed_values(&mut tokens);
    normalize_values_expression(&mut tokens, None);
    normalize_values_expression(&mut tokens, Some("map_from_entries"));
    normalize_scalar_values_relations(&mut tokens, dialect)?;
    normalize_trino_non_decimal_integer_literals(&mut tokens);
    validate_trino_identifiers(&tokens)?;
    normalize_iceberg_branch_references(&mut tokens, dialect);
    if let Some(token) = tokens.iter().find(|token| token.token == Token::AtSign) {
        return Err(ParserError::ParserError(format!(
            "unexpected @ outside an Iceberg DML branch reference at Line: {}, Column: {}",
            token.span.start.line, token.span.start.column
        )));
    }
    let parsed = Parser::new(dialect)
        .with_tokens_with_locations(tokens.clone())
        .parse_statements();
    if let Ok(statements) = parsed {
        return Ok(ParsedSql {
            statements,
            inline_functions: Vec::new(),
            compatibility_metadata: Vec::new(),
        });
    }
    normalize_materialized_view_options(&mut tokens);
    normalize_match_recognize_subsets(&mut tokens);
    normalize_with_session(&mut tokens, dialect);
    normalize_nested_row_types(&mut tokens);
    let inline_functions = normalize_inline_functions(&mut tokens, dialect)?;
    let row_expansions = normalize_row_expansions(&mut tokens, dialect)?;
    let statements = Parser::new(dialect)
        .with_tokens_with_locations(tokens)
        .parse_statements()?;
    Ok(ParsedSql {
        statements,
        inline_functions,
        compatibility_metadata: row_expansions,
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
            parsed.inline_functions[0],
            Statement::CreateFunction(_)
        ));
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

        let mut tokens = Tokenizer::new(&dialect, "SELECT 1 GROUP BY ROLLUP ()")
            .tokenize_with_location()
            .unwrap();
        let rollup = tokens
            .iter()
            .position(|token| is_unquoted_word(token, "rollup"))
            .unwrap();
        assert!(is_grouping_element_position(&tokens, rollup));
        normalize_empty_grouping_elements(&mut tokens);
        assert!(Parser::new(&dialect)
            .with_tokens_with_locations(tokens)
            .parse_statements()
            .is_ok());
        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY CUBE ()").is_ok());
        assert!(parse_sql(&dialect, "SELECT 1 GROUP BY GROUPING SETS ()").is_err());
    }

    #[test]
    fn at_local_requires_a_complete_modifier() {
        let dialect = TrinoDialect {};
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT LOCAL").is_ok());
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT").is_err());
        assert!(parse_sql(&dialect, "SELECT current_timestamp AT TIME").is_err());
    }

    #[test]
    fn scalar_values_relations_require_expressions() {
        let dialect = TrinoDialect {};
        assert!(parse_sql(&dialect, "SELECT * FROM LATERAL (VALUES 1, 2)").is_ok());
        assert!(parse_sql(&dialect, "SELECT * FROM LATERAL (VALUES )").is_err());
        assert!(parse_sql(&dialect, "INSERT INTO target VALUES 1").is_err());
        assert!(parse_sql(&dialect, "CREATE TABLE foo () AS (VALUES 1)").is_err());
        assert!(parse_sql(&dialect, "SELECT count(DISTINCT *) FROM (VALUES 1)").is_err());
    }
}
