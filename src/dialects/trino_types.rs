use std::cmp::Reverse;

use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer};

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
    let parsed = Parser::new(dialect)
        .with_tokens_with_locations(tokens.clone())
        .parse_statements();
    if parsed.is_ok() {
        return parsed;
    }
    normalize_nested_row_types(&mut tokens);
    Parser::new(dialect)
        .with_tokens_with_locations(tokens)
        .parse_statements()
}
