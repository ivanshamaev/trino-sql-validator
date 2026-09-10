use std::cmp::Reverse;

use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer, Whitespace};

use crate::types;

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

pub(crate) fn parse_sql(dialect: &dyn Dialect, sql: &str) -> Result<Vec<Statement>, ParserError> {
    let mut tokens = Tokenizer::new(dialect, sql).tokenize_with_location()?;
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
    if parsed.is_ok() {
        return parsed;
    }
    normalize_materialized_view_options(&mut tokens);
    normalize_match_recognize_subsets(&mut tokens);
    normalize_with_session(&mut tokens, dialect);
    normalize_nested_row_types(&mut tokens);
    Parser::new(dialect)
        .with_tokens_with_locations(tokens)
        .parse_statements()
}
