use sqlparser::ast::{CreateFunction, CreateFunctionBody, FunctionReturnType, Statement};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::Token;

/// Trino-only SQL statements that `sqlparser` has no AST for.
///
/// The grammar for these statements is shape-checked token by token. When the
/// tokens match a known Trino statement exactly, a placeholder ([`placeholder`])
/// is returned as the parsed statement so the caller can count it as a valid
/// statement. When they do not match, `None` is returned and the caller falls
/// back to the regular sqlparser statement parser.
pub(crate) fn try_parse_statement(p: &mut Parser) -> Option<Result<Statement, ParserError>> {
    let first = p.peek_token_ref().token.clone();
    let keyword = match first {
        Token::Word(w) => w.keyword,
        _ => return None,
    };
    let parsed = match keyword {
        Keyword::ALTER => {
            if is_trino_property_alter(p) {
                return Some(parse_alter(p));
            }
            p.maybe_parse(parse_alter).ok().flatten()
        }
        Keyword::CREATE => {
            // `CREATE FUNCTION` (Trino shape) is handled deterministically so a
            // missing `RETURNS`/`RETURN` reports an error instead of silently
            // falling through to sqlparser's (different) CREATE FUNCTION.
            if is_trino_create_function(p) {
                return Some(parse_create_function(p));
            }
            if is_trino_create(p) {
                return Some(parse_create(p));
            }
            p.maybe_parse(parse_create).ok().flatten()
        }
        Keyword::DESCRIBE => {
            // A plain `DESCRIBE <table>` goes to sqlparser; `DESCRIBE INPUT/
            // OUTPUT` is Trino-only and handled strictly so a missing name is
            // not mistaken for a table called `input`.
            if is_trino_describe(p) {
                return Some(parse_describe(p));
            }
            p.maybe_parse(parse_describe).ok().flatten()
        }
        Keyword::DROP => p.maybe_parse(parse_drop).ok().flatten(),
        Keyword::GRANT => p.maybe_parse(parse_grant_roles).ok().flatten(),
        Keyword::REFRESH => p
            .maybe_parse(parse_refresh_materialized_view)
            .ok()
            .flatten(),
        Keyword::RESET => {
            // `RESET SESSION` is Trino-only; handle strictly so a bare
            // `RESET SESSION` (no name, no AUTHORIZATION) is rejected rather
            // than accepted by sqlparser's permissive fallback.
            if is_trino_reset_session(p) {
                return Some(parse_reset_session(p));
            }
            p.maybe_parse(parse_reset_session).ok().flatten()
        }
        Keyword::REVOKE => p.maybe_parse(parse_revoke_roles).ok().flatten(),
        Keyword::SET => {
            if is_trino_set_path(p) {
                return Some(parse_set_path(p));
            }
            if is_trino_set_session_authorization(p) {
                return Some(parse_set_session_authorization(p));
            }
            p.maybe_parse(parse_set_path).ok().flatten()
        }
        Keyword::SHOW => {
            if is_trino_show(p) {
                return Some(parse_show(p));
            }
            p.maybe_parse(parse_show_create).ok().flatten()
        }
        Keyword::EXPLAIN if is_unquoted_word_at(p, 1, "verbose") => {
            return Some(Err(ParserError::ParserError(
                "EXPLAIN VERBOSE requires ANALYZE".into(),
            )));
        }
        _ => return None,
    };
    parsed.map(Ok)
}

/// `CREATE [OR REPLACE] [TEMP|TEMPORARY] FUNCTION <name> (`
fn is_trino_create_function(p: &mut Parser) -> bool {
    let mut index = 1;
    if is_unquoted_word_at(p, index, "or") {
        index += 1;
        if is_unquoted_word_at(p, index, "replace") {
            index += 1;
        }
    }
    if is_unquoted_word_at(p, index, "temporary") || is_unquoted_word_at(p, index, "temp") {
        index += 1;
    }
    is_unquoted_word_at(p, index, "function")
}

fn is_unquoted_word_at(p: &Parser, index: usize, expected: &str) -> bool {
    matches!(
        p.peek_nth_token(index).token,
        Token::Word(w) if w.value.eq_ignore_ascii_case(expected)
    )
}

fn is_trino_create(p: &Parser) -> bool {
    let mut index = 1;
    if is_unquoted_word_at(p, index, "or") {
        index += 1;
        if !is_unquoted_word_at(p, index, "replace") {
            return false;
        }
        index += 1;
    }
    is_unquoted_word_at(p, index, "branch") || is_unquoted_word_at(p, index, "catalog")
}

fn is_trino_property_alter(p: &Parser) -> bool {
    let mut index = match (
        is_unquoted_word_at(p, 1, "table"),
        is_unquoted_word_at(p, 1, "materialized"),
        is_unquoted_word_at(p, 1, "view"),
    ) {
        (true, _, _) => 2,
        (_, true, _) if is_unquoted_word_at(p, 2, "view") => 3,
        (_, _, true) => 2,
        _ => return false,
    };
    if is_unquoted_word_at(p, index, "if") {
        if !is_unquoted_word_at(p, index + 1, "exists") {
            return false;
        }
        index += 2;
    }
    if !matches!(p.peek_nth_token(index).token, Token::Word(_)) {
        return false;
    }
    index += 1;
    while p.peek_nth_token(index).token == Token::Period {
        if !matches!(p.peek_nth_token(index + 1).token, Token::Word(_)) {
            return false;
        }
        index += 2;
    }
    is_unquoted_word_at(p, index, "set") && is_unquoted_word_at(p, index + 1, "properties")
}

/// Placeholder returned for recognized Trino-only statements. Only the count
/// matters downstream; nothing inspects the contents.
fn placeholder() -> Statement {
    Statement::Commit {
        chain: false,
        end: false,
        modifier: None,
    }
}

fn opt_kw(p: &mut Parser, kw: Keyword) -> bool {
    p.parse_one_of_keywords(&[kw]).is_some()
}

fn consume_word(p: &mut Parser, word: &str) -> bool {
    match p.peek_token_ref().token.clone() {
        Token::Word(w) if w.value.eq_ignore_ascii_case(word) => {
            p.next_token();
            true
        }
        _ => false,
    }
}

fn end_of_statement(p: &mut Parser) -> Result<(), ParserError> {
    match p.peek_token_ref().token.clone() {
        Token::EOF | Token::SemiColon => Ok(()),
        other => Err(ParserError::ParserError(format!(
            "unexpected trailing token {other} in Trino statement"
        ))),
    }
}

/// Consume every token up to the end of the statement (a `;` or EOF at nesting
/// depth 0), keeping `()`, `[]` and `{}` balanced. The trailing `;` is left for
/// the caller.
fn consume_to_end(p: &mut Parser) -> Result<(), ParserError> {
    let mut depth: i64 = 0;
    loop {
        match p.peek_token_ref().token.clone() {
            Token::EOF => {
                if depth == 0 {
                    return Ok(());
                }
                return Err(ParserError::ParserError(
                    "unterminated group in Trino statement".into(),
                ));
            }
            Token::SemiColon if depth == 0 => return Ok(()),
            Token::LParen | Token::LBracket | Token::LBrace => {
                depth += 1;
                p.next_token();
            }
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth -= 1;
                if depth < 0 {
                    return Err(ParserError::ParserError(
                        "unbalanced closing bracket in Trino statement".into(),
                    ));
                }
                p.next_token();
            }
            _ => {
                p.next_token();
            }
        }
    }
}

/// Consume a balanced `(...)` group including its outer parentheses.
fn consume_balanced_parens(p: &mut Parser) -> Result<(), ParserError> {
    let mut depth = 1u32;
    loop {
        match p.peek_token_ref().token.clone() {
            Token::EOF => {
                return Err(ParserError::ParserError(
                    "unterminated group in Trino statement".into(),
                ))
            }
            Token::LParen => {
                depth += 1;
                p.next_token();
            }
            Token::RParen => {
                depth -= 1;
                p.next_token();
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => {
                p.next_token();
            }
        }
    }
}

/// Parse `key = value [, ...]` assignments for bare `SET PROPERTIES` clauses
/// and parenthesized `WITH (...)` clauses.
fn parse_properties(p: &mut Parser, parenthesized: bool) -> Result<(), ParserError> {
    if parenthesized {
        p.expect_token(&Token::LParen)?;
    } else if p.peek_token_ref().token == Token::LParen {
        return Err(ParserError::ParserError(
            "Trino SET PROPERTIES does not use parentheses".into(),
        ));
    }
    loop {
        p.parse_identifier()?;
        p.expect_token(&Token::Eq)?;
        if !consume_word(p, "default") {
            p.parse_expr()?;
        }
        if !p.consume_token(&Token::Comma) {
            break;
        }
    }
    if parenthesized {
        p.expect_token(&Token::RParen)?;
    }
    Ok(())
}

fn is_trino_reset_session(p: &mut Parser) -> bool {
    matches!(
        p.peek_nth_token(1).token,
        Token::Word(w) if w.value.eq_ignore_ascii_case("session")
    )
}

fn is_trino_set_path(p: &Parser) -> bool {
    is_unquoted_word_at(p, 1, "path")
}

fn is_trino_set_session_authorization(p: &Parser) -> bool {
    is_unquoted_word_at(p, 1, "session") && is_unquoted_word_at(p, 2, "authorization")
}

fn is_trino_show(p: &Parser) -> bool {
    [
        "catalogs",
        "schemas",
        "tables",
        "columns",
        "functions",
        "session",
    ]
    .iter()
    .any(|word| is_unquoted_word_at(p, 1, word))
}

fn parse_reset_session(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::RESET)?;
    p.expect_keyword(Keyword::SESSION)?;
    match p.peek_token_ref().token.clone() {
        Token::Word(w) if w.keyword == Keyword::AUTHORIZATION => {
            p.next_token();
        }
        _ => {
            p.parse_object_name(true)?;
        }
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_set_path(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::SET)?;
    p.expect_keyword(Keyword::PATH)?;
    parse_path_element(p)?;
    while p.consume_token(&Token::Comma) {
        parse_path_element(p)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_path_element(p: &mut Parser) -> Result<(), ParserError> {
    let path = p.parse_object_name(true)?;
    if path.0.len() > 2 {
        return Err(ParserError::ParserError(
            "Trino path element has at most catalog and schema".into(),
        ));
    }
    Ok(())
}

fn parse_set_session_authorization(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::SET)?;
    p.expect_keyword(Keyword::SESSION)?;
    p.expect_keyword(Keyword::AUTHORIZATION)?;
    match &p.peek_token_ref().token {
        Token::SingleQuotedString(_) | Token::UnicodeStringLiteral(_) => {
            p.next_token();
        }
        Token::Word(word) if word.value.eq_ignore_ascii_case("null") => {
            return Err(ParserError::ParserError(
                "SET SESSION AUTHORIZATION does not accept NULL".into(),
            ));
        }
        _ => {
            p.parse_identifier()?;
        }
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_show_create(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::SHOW)?;
    p.expect_keyword(Keyword::CREATE)?;
    if !consume_word(p, "schema") {
        p.expect_keyword(Keyword::MATERIALIZED)?;
        p.expect_keyword(Keyword::VIEW)?;
    }
    p.parse_object_name(true)?;
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_show(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::SHOW)?;
    if consume_word(p, "catalogs") {
        parse_show_like(p)?;
    } else if consume_word(p, "schemas") {
        parse_optional_show_scope(p, false)?;
        parse_show_like(p)?;
    } else if consume_word(p, "tables") {
        parse_optional_show_scope(p, true)?;
        parse_show_like(p)?;
    } else if consume_word(p, "columns") {
        parse_required_show_scope(p, true)?;
        parse_show_like(p)?;
    } else if consume_word(p, "functions") {
        parse_optional_show_scope(p, true)?;
        parse_show_like(p)?;
    } else if consume_word(p, "session") {
        parse_show_like(p)?;
    } else {
        return Err(ParserError::ParserError(
            "not a Trino SHOW statement".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_optional_show_scope(p: &mut Parser, qualified: bool) -> Result<(), ParserError> {
    if consume_word(p, "from") || consume_word(p, "in") {
        parse_show_scope(p, qualified)?;
    }
    Ok(())
}

fn parse_required_show_scope(p: &mut Parser, qualified: bool) -> Result<(), ParserError> {
    if !(consume_word(p, "from") || consume_word(p, "in")) {
        return Err(ParserError::ParserError(
            "Trino SHOW COLUMNS requires FROM or IN".into(),
        ));
    }
    parse_show_scope(p, qualified)
}

fn parse_show_scope(p: &mut Parser, qualified: bool) -> Result<(), ParserError> {
    if qualified {
        p.parse_object_name(true)?;
    } else {
        p.parse_identifier()?;
    }
    Ok(())
}

fn parse_show_like(p: &mut Parser) -> Result<(), ParserError> {
    if opt_kw(p, Keyword::LIKE) {
        parse_trino_string(p)?;
        if opt_kw(p, Keyword::ESCAPE) {
            parse_trino_string(p)?;
        }
    }
    Ok(())
}

fn is_trino_describe(p: &mut Parser) -> bool {
    matches!(
        p.peek_nth_token(1).token,
        Token::Word(w) if w.value.eq_ignore_ascii_case("input") || w.value.eq_ignore_ascii_case("output")
    )
}

fn parse_describe(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::DESCRIBE)?;
    if p.parse_one_of_keywords(&[Keyword::INPUT, Keyword::OUTPUT])
        .is_none()
    {
        return Err(ParserError::ParserError(
            "not a Trino DESCRIBE INPUT/OUTPUT statement".into(),
        ));
    }
    if p.consume_token(&Token::LParen) {
        consume_balanced_parens(p)?;
    } else {
        p.parse_object_name(true)?;
    }
    if opt_kw(p, Keyword::WHERE) {
        consume_to_end(p)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_refresh_materialized_view(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::REFRESH)?;
    p.expect_keyword(Keyword::MATERIALIZED)?;
    p.expect_keyword(Keyword::VIEW)?;
    p.parse_object_name(true)?;
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_create(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::CREATE)?;
    let or_replace = if opt_kw(p, Keyword::OR) {
        p.expect_keyword(Keyword::REPLACE)?;
        true
    } else {
        false
    };
    if consume_word(p, "branch") {
        return parse_create_branch(p, or_replace);
    }
    if or_replace {
        return Err(ParserError::ParserError(
            "CREATE OR REPLACE is only supported for Trino branches here".into(),
        ));
    }
    if opt_kw(p, Keyword::CATALOG) {
        return parse_create_catalog(p);
    }
    Err(ParserError::ParserError(
        "not a Trino-only CREATE statement".into(),
    ))
}

fn parse_create_catalog(p: &mut Parser) -> Result<Statement, ParserError> {
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::NOT)?;
        p.expect_keyword(Keyword::EXISTS)?;
    }
    p.parse_identifier()?;
    p.expect_keyword(Keyword::USING)?;
    p.parse_identifier()?;
    if opt_kw(p, Keyword::COMMENT) {
        parse_trino_string(p)?;
    }
    if opt_kw(p, Keyword::AUTHORIZATION) {
        parse_principal(p)?;
    }
    if opt_kw(p, Keyword::WITH) {
        parse_properties(p, true)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_trino_string(p: &mut Parser) -> Result<(), ParserError> {
    match &p.peek_token_ref().token {
        Token::SingleQuotedString(_) | Token::UnicodeStringLiteral(_) => {
            p.next_token();
            Ok(())
        }
        other => Err(ParserError::ParserError(format!(
            "Expected Trino string, found {other}"
        ))),
    }
}

fn parse_principal(p: &mut Parser) -> Result<(), ParserError> {
    opt_kw(p, Keyword::USER);
    opt_kw(p, Keyword::ROLE);
    p.parse_identifier()?;
    Ok(())
}

fn parse_create_function(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::CREATE)?;
    let mut or_replace = false;
    if opt_kw(p, Keyword::OR) {
        p.expect_keyword(Keyword::REPLACE)?;
        or_replace = true;
    }
    let temporary = if opt_kw(p, Keyword::TEMPORARY) {
        true
    } else {
        opt_kw(p, Keyword::TEMP)
    };
    p.expect_keyword(Keyword::FUNCTION)?;
    let name = p.parse_object_name(true)?;
    p.expect_token(&Token::LParen)?;
    consume_balanced_parens(p)?;
    p.expect_keyword(Keyword::RETURNS)?;
    let return_type = p.parse_data_type()?;
    // return type + any option clauses up to the mandatory `RETURN`
    // expression. `RETURNS` is a distinct keyword from `RETURN`, so scanning
    // for `RETURN` is unambiguous.
    loop {
        match p.peek_token_ref().token.clone() {
            Token::Word(w) if w.keyword == Keyword::RETURN => {
                p.next_token();
                let body = p.parse_expr()?;
                end_of_statement(p)?;
                return Ok(Statement::CreateFunction(CreateFunction {
                    or_alter: false,
                    or_replace,
                    temporary,
                    if_not_exists: false,
                    name,
                    args: None,
                    return_type: Some(FunctionReturnType::DataType(return_type)),
                    function_body: Some(CreateFunctionBody::Return(body)),
                    behavior: None,
                    called_on_null: None,
                    parallel: None,
                    security: None,
                    set_params: Vec::new(),
                    using: None,
                    language: None,
                    determinism_specifier: None,
                    options: None,
                    remote_connection: None,
                }));
            }
            Token::EOF | Token::SemiColon => {
                return Err(ParserError::ParserError(
                    "CREATE FUNCTION is missing its RETURN expression".into(),
                ))
            }
            _ => {
                p.next_token();
            }
        }
    }
}

fn parse_create_branch(p: &mut Parser, or_replace: bool) -> Result<Statement, ParserError> {
    let if_not_exists = if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::NOT)?;
        p.expect_keyword(Keyword::EXISTS)?;
        true
    } else {
        false
    };
    if or_replace && if_not_exists {
        return Err(ParserError::ParserError(
            "CREATE BRANCH cannot combine OR REPLACE with IF NOT EXISTS".into(),
        ));
    }
    p.parse_identifier()?;
    if opt_kw(p, Keyword::WITH) {
        parse_properties(p, true)?;
    }
    p.expect_keyword(Keyword::IN)?;
    p.expect_keyword(Keyword::TABLE)?;
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::FROM) {
        p.parse_identifier()?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_drop(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::DROP)?;
    let is_catalog = if opt_kw(p, Keyword::CATALOG) {
        true
    } else if consume_word(p, "branch") {
        false
    } else {
        return Err(ParserError::ParserError(
            "not a Trino-only DROP statement".into(),
        ));
    };
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
    }
    if is_catalog {
        p.parse_identifier()?;
        if !opt_kw(p, Keyword::CASCADE) {
            opt_kw(p, Keyword::RESTRICT);
        }
    } else {
        p.parse_identifier()?;
        p.expect_keyword(Keyword::IN)?;
        p.expect_keyword(Keyword::TABLE)?;
        p.parse_object_name(true)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_alter(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::ALTER)?;
    if opt_kw(p, Keyword::TABLE) {
        return parse_alter_table(p);
    }
    if opt_kw(p, Keyword::MATERIALIZED) {
        p.expect_keyword(Keyword::VIEW)?;
        return parse_alter_materialized_view(p);
    }
    if opt_kw(p, Keyword::VIEW) {
        return parse_alter_view(p);
    }
    if consume_word(p, "branch") {
        return parse_alter_branch(p);
    }
    Err(ParserError::ParserError(
        "not a Trino-only ALTER statement".into(),
    ))
}

fn parse_alter_table(p: &mut Parser) -> Result<Statement, ParserError> {
    let if_exists = if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
        true
    } else {
        false
    };
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::SET) {
        if consume_word(p, "properties") {
            if if_exists {
                return Err(ParserError::ParserError(
                    "ALTER TABLE SET PROPERTIES does not support IF EXISTS".into(),
                ));
            }
            parse_properties(p, false)?;
        } else if opt_kw(p, Keyword::AUTHORIZATION) {
            consume_to_end(p)?;
        } else {
            return Err(ParserError::ParserError(
                "unsupported ALTER TABLE ... SET form".into(),
            ));
        }
    } else if opt_kw(p, Keyword::EXECUTE) {
        p.parse_object_name(true)?;
        if p.consume_token(&Token::LParen) {
            consume_balanced_parens(p)?;
        }
        if opt_kw(p, Keyword::WHERE) {
            consume_to_end(p)?;
        }
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER TABLE form".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_alter_materialized_view(p: &mut Parser) -> Result<Statement, ParserError> {
    let if_exists = if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
        true
    } else {
        false
    };
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::RENAME) {
        p.expect_keyword(Keyword::TO)?;
        p.parse_object_name(true)?;
    } else if opt_kw(p, Keyword::SET) {
        if consume_word(p, "properties") {
            if if_exists {
                return Err(ParserError::ParserError(
                    "ALTER MATERIALIZED VIEW SET PROPERTIES does not support IF EXISTS".into(),
                ));
            }
            parse_properties(p, false)?;
        } else if opt_kw(p, Keyword::AUTHORIZATION) {
            consume_to_end(p)?;
        } else {
            return Err(ParserError::ParserError(
                "unsupported ALTER MATERIALIZED VIEW ... SET form".into(),
            ));
        }
    } else if opt_kw(p, Keyword::EXECUTE) {
        p.parse_object_name(true)?;
        if p.consume_token(&Token::LParen) {
            consume_balanced_parens(p)?;
        }
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER MATERIALIZED VIEW form".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_alter_view(p: &mut Parser) -> Result<Statement, ParserError> {
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::RENAME) {
        p.expect_keyword(Keyword::TO)?;
        p.parse_object_name(true)?;
    } else if opt_kw(p, Keyword::REFRESH) {
        // ALTER VIEW name REFRESH — nothing else to consume
    } else if opt_kw(p, Keyword::SET) {
        if opt_kw(p, Keyword::AUTHORIZATION) {
            consume_to_end(p)?;
        } else {
            return Err(ParserError::ParserError(
                "unsupported ALTER VIEW ... SET form".into(),
            ));
        }
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER VIEW form".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_alter_branch(p: &mut Parser) -> Result<Statement, ParserError> {
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::IN) {
        p.expect_keyword(Keyword::TABLE)?;
        p.parse_object_name(true)?;
        consume_word(p, "fast");
        consume_word(p, "forward");
        p.expect_keyword(Keyword::TO)?;
        p.parse_object_name(true)?;
    } else if opt_kw(p, Keyword::SET) {
        if !consume_word(p, "retention") {
            return Err(ParserError::ParserError(
                "unsupported ALTER BRANCH ... SET form".into(),
            ));
        }
        if matches!(p.peek_token_ref().token, Token::EOF | Token::SemiColon) {
            return Err(ParserError::ParserError(
                "ALTER BRANCH SET RETENTION is missing its retention period".into(),
            ));
        }
        consume_to_end(p)?;
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER BRANCH form".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_grant_roles(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::GRANT)?;
    loop {
        p.parse_object_name(true)?;
        if !p.consume_token(&Token::Comma) {
            break;
        }
    }
    p.expect_keyword(Keyword::TO)?;
    loop {
        opt_kw(p, Keyword::USER);
        opt_kw(p, Keyword::ROLE);
        p.parse_object_name(true)?;
        if !p.consume_token(&Token::Comma) {
            break;
        }
    }
    if opt_kw(p, Keyword::WITH) {
        p.expect_keyword(Keyword::ADMIN)?;
        p.expect_keyword(Keyword::OPTION)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_revoke_roles(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::REVOKE)?;
    // `ADMIN` is only the admin-option marker when followed by `OPTION`;
    // otherwise it is a role name (e.g. `REVOKE admin FROM ...`).
    if p.peek_keyword(Keyword::ADMIN)
        && matches!(
            p.peek_nth_token(1).token,
            Token::Word(ref w) if w.keyword == Keyword::OPTION
        )
    {
        p.next_token();
        p.expect_keyword(Keyword::OPTION)?;
        opt_kw(p, Keyword::FOR);
    }
    loop {
        p.parse_object_name(true)?;
        if !p.consume_token(&Token::Comma) {
            break;
        }
    }
    p.expect_keyword(Keyword::FROM)?;
    loop {
        opt_kw(p, Keyword::USER);
        opt_kw(p, Keyword::ROLE);
        p.parse_object_name(true)?;
        if !p.consume_token(&Token::Comma) {
            break;
        }
    }
    end_of_statement(p)?;
    Ok(placeholder())
}
#[cfg(test)]
mod tests {
    use sqlparser::parser::Parser;

    fn parses(sql: &str) -> bool {
        let trino = super::super::TrinoDialect {};
        Parser::parse_sql(&trino, sql).is_ok()
    }

    #[test]
    fn valid_trino_only_statements_parse() {
        for sql in [
            "RESET SESSION AUTHORIZATION",
            "RESET SESSION authorize",
            "SET PATH a, b",
            "SHOW CREATE SCHEMA s",
            "SHOW CREATE MATERIALIZED VIEW mv",
            "SHOW CATALOGS LIKE '%$_%' ESCAPE '$'",
            "SHOW SCHEMAS IN hive LIKE '%$_%' ESCAPE '$'",
            "SHOW TABLES FROM hive.default LIKE '%$_%' ESCAPE '$'",
            "SHOW COLUMNS FROM hive.default.orders LIKE '%$_%' ESCAPE '$'",
            "SHOW FUNCTIONS FROM hive.default LIKE '%$_%' ESCAPE '$'",
            "SHOW SESSION LIKE '%$_%' ESCAPE '$'",
            "DESCRIBE INPUT stmt",
            "DESCRIBE OUTPUT my_query",
            "DESCRIBE OUTPUT my_query WHERE output = 1",
            "REFRESH MATERIALIZED VIEW mv",
            "CREATE CATALOG hive USING hive",
            "CREATE CATALOG IF NOT EXISTS hive USING hive WITH (\"hive.metastore.uri\" = 'thrift://host:9083')",
            "CREATE CATALOG test USING conn COMMENT 'awesome' AUTHORIZATION ROLE dragon WITH (\"a\" = 'apple', \"b\" = 123)",
            "DROP CATALOG hive",
            "DROP CATALOG hive CASCADE",
            "DROP CATALOG IF EXISTS hive RESTRICT",
            "CREATE BRANCH b1 IN TABLE t",
            "CREATE OR REPLACE BRANCH b2 IN TABLE t FROM b1",
            "DROP BRANCH b1 IN TABLE t",
            "ALTER BRANCH b1 IN TABLE t FAST FORWARD TO t2",
            "ALTER BRANCH b2 SET RETENTION 3 DAYS",
            "ALTER TABLE t SET PROPERTIES x = 1",
            "ALTER TABLE t SET PROPERTIES x = 'v', loc = 's3://b'",
            "ALTER TABLE t SET AUTHORIZATION ROLE role1",
            "ALTER TABLE t SET AUTHORIZATION USER user1",
            "ALTER TABLE t EXECUTE optimize",
            "ALTER TABLE t EXECUTE optimize (file_size_threshold = '16MB')",
            "ALTER VIEW v RENAME TO v2",
            "ALTER VIEW v REFRESH",
            "ALTER VIEW v SET AUTHORIZATION ROLE r",
            "ALTER MATERIALIZED VIEW mv RENAME TO mv2",
            "ALTER MATERIALIZED VIEW mv SET PROPERTIES p = 'q'",
            "ALTER MATERIALIZED VIEW mv EXECUTE refresh",
            "GRANT admin TO user1",
            "REVOKE admin FROM user1",
            "GRANT role1 TO USER u, ROLE r WITH ADMIN OPTION",
            "REVOKE ADMIN OPTION FOR role1 FROM USER u",
            "CREATE FUNCTION testing.default.add_two(x bigint) RETURNS bigint COMMENT 'x' LANGUAGE SQL DETERMINISTIC RETURNS NULL ON NULL INPUT RETURN x + 2",
            "CREATE OR REPLACE FUNCTION f() RETURNS bigint RETURN 1",
        ] {
            assert!(parses(sql), "expected to parse: {sql}");
        }
    }

    #[test]
    fn invalid_trino_only_statements_are_rejected() {
        for sql in [
            "RESET SESSION",
            "DESCRIBE INPUT",
            "REFRESH MATERIALIZED VIEW",
            "CREATE CATALOG",
            "CREATE CATALOG hive",
            "CREATE OR REPLACE CATALOG hive USING hive",
            "CREATE CATALOG hive USING hive (x = 1)",
            "DROP BRANCH",
            "DROP BRANCH audit",
            "CREATE BRANCH b",
            "CREATE OR REPLACE BRANCH IF NOT EXISTS b IN TABLE t",
            "CREATE FUNCTION f()",
            "CREATE FUNCTION f() RETURNS bigint COMMENT 'x'",
            "ALTER TABLE t SET PROPERTIES",
            "ALTER TABLE t SET PROPERTIES ()",
            "ALTER TABLE t SET PROPERTIES (x = )",
            "ALTER TABLE t SET PROPERTIES x = ",
            "ALTER TABLE t SET PROPERTIES (x = 1)",
            "ALTER TABLE IF EXISTS t SET PROPERTIES x = 1",
            "ALTER MATERIALIZED VIEW t SET PROPERTIES (x = 1)",
            "ALTER MATERIALIZED VIEW IF EXISTS t SET PROPERTIES x = 1",
            "ALTER VIEW t SET PROPERTIES x = 1",
            "ALTER BRANCH b SET",
            "ALTER TABLE t EXECUTE",
            "SET PATH one.too.many, qualifiers",
            "SET SESSION AUTHORIZATION null",
            "EXPLAIN VERBOSE SELECT * FROM t",
            "SHOW SESSION LIKE '%$_%' ESCAPE",
            "SHOW COLUMNS orders",
        ] {
            assert!(!parses(sql), "expected to reject: {sql}");
        }
    }

    #[test]
    fn valid_generic_statements_still_parse() {
        for sql in [
            "SELECT 1",
            "CREATE TABLE t (a bigint)",
            "ALTER TABLE t ADD COLUMN c varchar",
        ] {
            assert!(parses(sql), "expected to parse: {sql}");
        }
    }
}
