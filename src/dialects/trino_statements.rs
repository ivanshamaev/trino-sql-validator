use sqlparser::ast::{
    CreateFunction, CreateFunctionBody, Expr, FunctionCalledOnNull, FunctionDeterminismSpecifier,
    FunctionReturnType, FunctionSecurity, OperateFunctionArg, Statement, UnaryOperator, Value,
};
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
    match keyword {
        Keyword::ALTER => {
            if is_trino_alter(p) || is_trino_owned_entity_alter(p) {
                Some(parse_located(p, parse_alter))
            } else {
                None
            }
        }
        Keyword::CREATE => {
            // `CREATE FUNCTION` (Trino shape) is handled deterministically so a
            // missing `RETURNS`/`RETURN` reports an error instead of silently
            // falling through to sqlparser's (different) CREATE FUNCTION.
            if is_trino_create_function(p) {
                Some(parse_located(p, parse_create_function))
            } else if is_trino_create(p) {
                Some(parse_located(p, parse_create))
            } else {
                None
            }
        }
        Keyword::DESCRIBE => {
            // A plain `DESCRIBE <table>` goes to sqlparser; `DESCRIBE INPUT/
            // OUTPUT` is Trino-only and handled strictly so a missing name is
            // not mistaken for a table called `input`.
            if is_trino_describe(p) {
                Some(parse_located(p, parse_describe))
            } else {
                None
            }
        }
        Keyword::DENY => Some(parse_located(p, parse_deny)),
        Keyword::DROP => {
            if is_trino_drop(p) {
                Some(parse_located(p, parse_drop))
            } else {
                None
            }
        }
        Keyword::GRANT => Some(parse_located(p, parse_grant)),
        Keyword::REFRESH if is_trino_refresh_materialized_view(p) => {
            Some(parse_located(p, parse_refresh_materialized_view))
        }
        Keyword::RESET => {
            // `RESET SESSION` is Trino-only; handle strictly so a bare
            // `RESET SESSION` (no name, no AUTHORIZATION) is rejected rather
            // than accepted by sqlparser's permissive fallback.
            if is_trino_reset_session(p) {
                Some(parse_located(p, parse_reset_session))
            } else {
                None
            }
        }
        Keyword::REVOKE => Some(parse_located(p, parse_revoke)),
        Keyword::SET => {
            if is_trino_set_role(p) {
                Some(parse_located(p, parse_set_role))
            } else if is_trino_set_path(p) {
                Some(parse_located(p, parse_set_path))
            } else if is_trino_set_session_authorization(p) {
                Some(parse_located(p, parse_set_session_authorization))
            } else {
                None
            }
        }
        Keyword::SHOW => {
            if is_trino_show(p) {
                Some(parse_located(p, parse_show))
            } else if is_trino_show_create(p) {
                Some(parse_located(p, parse_show_create))
            } else {
                None
            }
        }
        Keyword::EXPLAIN if is_unquoted_word_at(p, 1, "verbose") => {
            let location = p.peek_nth_token(1).span.start;
            Some(Err(ParserError::ParserError(format!(
                "EXPLAIN VERBOSE requires ANALYZE at Line: {}, Column: {}",
                location.line, location.column
            ))))
        }
        _ => None,
    }
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

fn parse_located(
    p: &mut Parser,
    parse: fn(&mut Parser) -> Result<Statement, ParserError>,
) -> Result<Statement, ParserError> {
    let result = parse(p);
    result.map_err(|error| match error {
        ParserError::ParserError(message) if !message.contains("at Line:") => {
            let location = p.peek_token_ref().span.start;
            ParserError::ParserError(format!(
                "{message} at Line: {}, Column: {}",
                location.line, location.column
            ))
        }
        other => other,
    })
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
    is_unquoted_word_at(p, index, "branch")
        || is_unquoted_word_at(p, index, "catalog")
        || is_unquoted_word_at(p, index, "role")
}

fn is_trino_drop(p: &Parser) -> bool {
    is_unquoted_word_at(p, 1, "branch")
        || is_unquoted_word_at(p, 1, "catalog")
        || is_unquoted_word_at(p, 1, "role")
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

fn is_trino_alter(p: &Parser) -> bool {
    is_trino_property_alter(p)
        || is_unquoted_word_at(p, 1, "table")
        || is_unquoted_word_at(p, 1, "view")
        || is_unquoted_word_at(p, 1, "branch")
        || (is_unquoted_word_at(p, 1, "materialized") && is_unquoted_word_at(p, 2, "view"))
}

fn is_trino_owned_entity_alter(p: &Parser) -> bool {
    if !matches!(p.peek_nth_token(1).token, Token::Word(_))
        || !matches!(p.peek_nth_token(2).token, Token::Word(_))
    {
        return false;
    }
    let mut index = 3;
    while p.peek_nth_token(index).token == Token::Period {
        if !matches!(p.peek_nth_token(index + 1).token, Token::Word(_)) {
            return false;
        }
        index += 2;
    }
    is_unquoted_word_at(p, index, "set") && is_unquoted_word_at(p, index + 1, "authorization")
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

fn is_trino_set_role(p: &Parser) -> bool {
    is_unquoted_word_at(p, 1, "role")
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

fn is_trino_show_create(p: &Parser) -> bool {
    is_unquoted_word_at(p, 1, "create")
        && (is_unquoted_word_at(p, 2, "schema")
            || (is_unquoted_word_at(p, 2, "materialized") && is_unquoted_word_at(p, 3, "view")))
}

fn is_trino_refresh_materialized_view(p: &Parser) -> bool {
    is_unquoted_word_at(p, 1, "materialized") && is_unquoted_word_at(p, 2, "view")
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
        Token::Word(word)
            if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("null") =>
        {
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

fn parse_set_role(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::SET)?;
    p.expect_keyword(Keyword::ROLE)?;
    if !(opt_kw(p, Keyword::ALL) || opt_kw(p, Keyword::NONE)) {
        p.parse_identifier()?;
    }
    if opt_kw(p, Keyword::IN) {
        p.parse_identifier()?;
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
    let query = if p.consume_token(&Token::LParen) {
        let query = p.parse_query()?;
        p.expect_token(&Token::RParen)?;
        Some(query)
    } else {
        p.parse_object_name(true)?;
        None
    };
    if opt_kw(p, Keyword::WHERE) {
        p.parse_expr()?;
    }
    end_of_statement(p)?;
    Ok(query.map_or_else(placeholder, Statement::Query))
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
    if opt_kw(p, Keyword::ROLE) {
        return parse_create_role(p);
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
    if !opt_kw(p, Keyword::USER) {
        opt_kw(p, Keyword::ROLE);
    }
    p.parse_identifier()?;
    Ok(())
}

fn parse_grantor(p: &mut Parser) -> Result<(), ParserError> {
    if opt_kw(p, Keyword::CURRENT_USER) || opt_kw(p, Keyword::CURRENT_ROLE) {
        return Ok(());
    }
    parse_principal(p)
}

fn parse_create_role(p: &mut Parser) -> Result<Statement, ParserError> {
    p.parse_identifier()?;
    if opt_kw(p, Keyword::WITH) {
        p.expect_keyword(Keyword::ADMIN)?;
        parse_grantor(p)?;
    }
    if opt_kw(p, Keyword::IN) {
        p.parse_identifier()?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_routine_arguments(p: &mut Parser) -> Result<Vec<OperateFunctionArg>, ParserError> {
    p.expect_token(&Token::LParen)?;
    let mut arguments = Vec::new();
    if p.consume_token(&Token::RParen) {
        return Ok(arguments);
    }
    loop {
        let named = p.maybe_parse(|p| {
            let name = p.parse_identifier()?;
            let data_type = p.parse_data_type()?;
            if !matches!(p.peek_token_ref().token, Token::Comma | Token::RParen) {
                return Err(ParserError::ParserError(
                    "unexpected token after routine parameter type".into(),
                ));
            }
            Ok(OperateFunctionArg {
                mode: None,
                name: Some(name),
                data_type,
                default_expr: None,
            })
        })?;
        arguments.push(match named {
            Some(argument) => argument,
            None => OperateFunctionArg::unnamed(p.parse_data_type()?),
        });
        if p.consume_token(&Token::Comma) {
            if p.peek_token_ref().token == Token::RParen {
                return Err(ParserError::ParserError(
                    "routine parameter list cannot end with a comma".into(),
                ));
            }
            continue;
        }
        p.expect_token(&Token::RParen)?;
        return Ok(arguments);
    }
}

fn at_control_terminator(p: &Parser, words: &[&str]) -> bool {
    words.iter().any(|word| is_unquoted_word_at(p, 0, word))
}

fn parse_control_list(
    p: &mut Parser,
    terminators: &[&str],
    arguments: &mut Vec<OperateFunctionArg>,
    expressions: &mut Vec<Expr>,
    required: bool,
) -> Result<(), ParserError> {
    let mut count = 0usize;
    while !at_control_terminator(p, terminators) {
        if matches!(p.peek_token_ref().token, Token::EOF | Token::SemiColon) {
            return Err(ParserError::ParserError(
                "unterminated SQL routine control statement".into(),
            ));
        }
        parse_control_statement(p, arguments, expressions)?;
        p.expect_token(&Token::SemiColon)?;
        count += 1;
    }
    if required && count == 0 {
        return Err(ParserError::ParserError(
            "SQL routine control block requires a statement".into(),
        ));
    }
    Ok(())
}

fn parse_variable_declaration(
    p: &mut Parser,
    arguments: &mut Vec<OperateFunctionArg>,
    expressions: &mut Vec<Expr>,
) -> Result<(), ParserError> {
    let mut names = vec![p.parse_identifier()?];
    while p.consume_token(&Token::Comma) {
        names.push(p.parse_identifier()?);
    }
    let data_type = p.parse_data_type()?;
    if consume_word(p, "default") {
        expressions.push(p.parse_expr()?);
    }
    p.expect_token(&Token::SemiColon)?;
    for name in names {
        arguments.push(OperateFunctionArg {
            mode: None,
            name: Some(name),
            data_type: data_type.clone(),
            default_expr: None,
        });
    }
    Ok(())
}

fn consume_control_label(p: &mut Parser) -> Result<(), ParserError> {
    if matches!(p.peek_token_ref().token, Token::Word(_))
        && p.peek_nth_token(1).token == Token::Colon
    {
        p.parse_identifier()?;
        p.expect_token(&Token::Colon)?;
    }
    Ok(())
}

fn parse_control_statement(
    p: &mut Parser,
    arguments: &mut Vec<OperateFunctionArg>,
    expressions: &mut Vec<Expr>,
) -> Result<(), ParserError> {
    if consume_word(p, "return") {
        expressions.push(p.parse_expr()?);
        return Ok(());
    }
    if consume_word(p, "set") {
        p.parse_identifier()?;
        p.expect_token(&Token::Eq)?;
        expressions.push(p.parse_expr()?);
        return Ok(());
    }
    if consume_word(p, "iterate") || consume_word(p, "leave") {
        p.parse_identifier()?;
        return Ok(());
    }
    if consume_word(p, "begin") {
        while consume_word(p, "declare") {
            parse_variable_declaration(p, arguments, expressions)?;
        }
        parse_control_list(p, &["end"], arguments, expressions, false)?;
        if !consume_word(p, "end") {
            return Err(ParserError::ParserError(
                "compound routine body requires END".into(),
            ));
        }
        return Ok(());
    }
    if consume_word(p, "if") {
        expressions.push(p.parse_expr()?);
        if !consume_word(p, "then") {
            return Err(ParserError::ParserError("IF requires THEN".into()));
        }
        parse_control_list(p, &["elseif", "else", "end"], arguments, expressions, true)?;
        while consume_word(p, "elseif") {
            expressions.push(p.parse_expr()?);
            if !consume_word(p, "then") {
                return Err(ParserError::ParserError("ELSEIF requires THEN".into()));
            }
            parse_control_list(p, &["elseif", "else", "end"], arguments, expressions, true)?;
        }
        if consume_word(p, "else") {
            parse_control_list(p, &["end"], arguments, expressions, true)?;
        }
        if !consume_word(p, "end") || !consume_word(p, "if") {
            return Err(ParserError::ParserError("IF requires END IF".into()));
        }
        return Ok(());
    }
    if consume_word(p, "case") {
        if !is_unquoted_word_at(p, 0, "when") {
            expressions.push(p.parse_expr()?);
        }
        let mut branches = 0usize;
        while consume_word(p, "when") {
            expressions.push(p.parse_expr()?);
            if !consume_word(p, "then") {
                return Err(ParserError::ParserError("CASE WHEN requires THEN".into()));
            }
            parse_control_list(p, &["when", "else", "end"], arguments, expressions, true)?;
            branches += 1;
        }
        if branches == 0 {
            return Err(ParserError::ParserError("CASE requires WHEN".into()));
        }
        if consume_word(p, "else") {
            parse_control_list(p, &["end"], arguments, expressions, true)?;
        }
        if !consume_word(p, "end") || !consume_word(p, "case") {
            return Err(ParserError::ParserError("CASE requires END CASE".into()));
        }
        return Ok(());
    }

    consume_control_label(p)?;
    if consume_word(p, "loop") {
        parse_control_list(p, &["end"], arguments, expressions, true)?;
        if !consume_word(p, "end") || !consume_word(p, "loop") {
            return Err(ParserError::ParserError("LOOP requires END LOOP".into()));
        }
        return Ok(());
    }
    if consume_word(p, "while") {
        expressions.push(p.parse_expr()?);
        if !consume_word(p, "do") {
            return Err(ParserError::ParserError("WHILE requires DO".into()));
        }
        parse_control_list(p, &["end"], arguments, expressions, true)?;
        if !consume_word(p, "end") || !consume_word(p, "while") {
            return Err(ParserError::ParserError("WHILE requires END WHILE".into()));
        }
        return Ok(());
    }
    if consume_word(p, "repeat") {
        parse_control_list(p, &["until"], arguments, expressions, true)?;
        if !consume_word(p, "until") {
            return Err(ParserError::ParserError("REPEAT requires UNTIL".into()));
        }
        expressions.push(p.parse_expr()?);
        if !consume_word(p, "end") || !consume_word(p, "repeat") {
            return Err(ParserError::ParserError(
                "REPEAT requires END REPEAT".into(),
            ));
        }
        return Ok(());
    }
    Err(ParserError::ParserError(
        "expected a SQL routine control statement".into(),
    ))
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
    let mut args = parse_routine_arguments(p)?;
    p.expect_keyword(Keyword::RETURNS)?;
    let return_type = p.parse_data_type()?;
    let mut language = None;
    let mut determinism_specifier = None;
    let mut called_on_null = None;
    let mut security = None;
    let mut comment = false;
    let function_body;
    loop {
        if consume_word(p, "return") {
            function_body = Some(CreateFunctionBody::Return(p.parse_expr()?));
            break;
        }
        if is_unquoted_word_at(p, 0, "begin") {
            let mut expressions = Vec::new();
            parse_control_statement(p, &mut args, &mut expressions)?;
            function_body = Some(CreateFunctionBody::Return(Expr::Tuple(expressions)));
            break;
        }
        if consume_word(p, "as") {
            let token = p.next_token();
            let Token::DollarQuotedString(body) = token.token else {
                return Err(ParserError::ParserError(
                    "AS routine body requires an untagged dollar string".into(),
                ));
            };
            if body.tag.is_some() {
                return Err(ParserError::ParserError(
                    "Trino routine dollar bodies do not support tags".into(),
                ));
            }
            function_body = Some(CreateFunctionBody::AsBeforeOptions {
                body: Expr::Value(Value::DollarQuotedString(body).with_span(token.span)),
                link_symbol: None,
            });
            break;
        }
        if consume_word(p, "language") {
            if language.is_some() {
                return Err(ParserError::ParserError(
                    "duplicate LANGUAGE routine characteristic".into(),
                ));
            }
            language = Some(p.parse_identifier()?);
            continue;
        }
        if consume_word(p, "not") {
            if !consume_word(p, "deterministic") || determinism_specifier.is_some() {
                return Err(ParserError::ParserError(
                    "invalid NOT DETERMINISTIC routine characteristic".into(),
                ));
            }
            determinism_specifier = Some(FunctionDeterminismSpecifier::NotDeterministic);
            continue;
        }
        if consume_word(p, "deterministic") {
            if determinism_specifier.is_some() {
                return Err(ParserError::ParserError(
                    "duplicate DETERMINISTIC routine characteristic".into(),
                ));
            }
            determinism_specifier = Some(FunctionDeterminismSpecifier::Deterministic);
            continue;
        }
        if consume_word(p, "returns") {
            if called_on_null.is_some()
                || !consume_word(p, "null")
                || !consume_word(p, "on")
                || !consume_word(p, "null")
                || !consume_word(p, "input")
            {
                return Err(ParserError::ParserError(
                    "invalid RETURNS NULL ON NULL INPUT routine characteristic".into(),
                ));
            }
            called_on_null = Some(FunctionCalledOnNull::ReturnsNullOnNullInput);
            continue;
        }
        if consume_word(p, "called") {
            if called_on_null.is_some()
                || !consume_word(p, "on")
                || !consume_word(p, "null")
                || !consume_word(p, "input")
            {
                return Err(ParserError::ParserError(
                    "invalid CALLED ON NULL INPUT routine characteristic".into(),
                ));
            }
            called_on_null = Some(FunctionCalledOnNull::CalledOnNullInput);
            continue;
        }
        if consume_word(p, "security") {
            if security.is_some() {
                return Err(ParserError::ParserError(
                    "duplicate SECURITY routine characteristic".into(),
                ));
            }
            security = if consume_word(p, "definer") {
                Some(FunctionSecurity::Definer)
            } else if consume_word(p, "invoker") {
                Some(FunctionSecurity::Invoker)
            } else {
                return Err(ParserError::ParserError(
                    "SECURITY requires DEFINER or INVOKER".into(),
                ));
            };
            continue;
        }
        if consume_word(p, "comment") {
            if comment {
                return Err(ParserError::ParserError(
                    "duplicate COMMENT routine characteristic".into(),
                ));
            }
            parse_trino_string(p)?;
            comment = true;
            continue;
        }
        if consume_word(p, "with") {
            parse_properties(p, true)?;
            continue;
        }
        if matches!(p.peek_token_ref().token, Token::EOF | Token::SemiColon) {
            return Err(ParserError::ParserError(
                "CREATE FUNCTION is missing its routine body".into(),
            ));
        }
        return Err(ParserError::ParserError(
            "invalid CREATE FUNCTION routine characteristic or body".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(Statement::CreateFunction(CreateFunction {
        or_alter: false,
        or_replace,
        temporary,
        if_not_exists: false,
        name,
        args: Some(args),
        return_type: Some(FunctionReturnType::DataType(return_type)),
        function_body,
        behavior: None,
        called_on_null,
        parallel: None,
        security,
        set_params: Vec::new(),
        using: None,
        language,
        determinism_specifier,
        options: None,
        remote_connection: None,
    }))
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
    enum DropKind {
        Catalog,
        Branch,
        Role,
    }
    let kind = if opt_kw(p, Keyword::CATALOG) {
        DropKind::Catalog
    } else if consume_word(p, "branch") {
        DropKind::Branch
    } else if opt_kw(p, Keyword::ROLE) {
        DropKind::Role
    } else {
        return Err(ParserError::ParserError(
            "not a Trino-only DROP statement".into(),
        ));
    };
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
    }
    match kind {
        DropKind::Catalog => {
            p.parse_identifier()?;
            if !opt_kw(p, Keyword::CASCADE) {
                opt_kw(p, Keyword::RESTRICT);
            }
        }
        DropKind::Branch => {
            p.parse_identifier()?;
            p.expect_keyword(Keyword::IN)?;
            p.expect_keyword(Keyword::TABLE)?;
            p.parse_object_name(true)?;
        }
        DropKind::Role => {
            p.parse_identifier()?;
            if opt_kw(p, Keyword::IN) {
                p.parse_identifier()?;
            }
        }
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
    p.parse_identifier()?;
    p.parse_object_name(true)?;
    p.expect_keyword(Keyword::SET)?;
    p.expect_keyword(Keyword::AUTHORIZATION)?;
    parse_principal(p)?;
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_alter_table(p: &mut Parser) -> Result<Statement, ParserError> {
    let if_exists = if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
        true
    } else {
        false
    };
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::RENAME) {
        if opt_kw(p, Keyword::COLUMN) {
            if opt_kw(p, Keyword::IF) {
                p.expect_keyword(Keyword::EXISTS)?;
            }
            p.parse_object_name(true)?;
            p.expect_keyword(Keyword::TO)?;
            p.parse_identifier()?;
        } else {
            p.expect_keyword(Keyword::TO)?;
            p.parse_object_name(true)?;
        }
    } else if opt_kw(p, Keyword::ADD) {
        p.expect_keyword(Keyword::COLUMN)?;
        if opt_kw(p, Keyword::IF) {
            p.expect_keyword(Keyword::NOT)?;
            p.expect_keyword(Keyword::EXISTS)?;
        }
        p.parse_object_name(true)?;
        p.parse_data_type()?;
        parse_column_options(p)?;
        if !(opt_kw(p, Keyword::FIRST) || consume_word(p, "last")) && opt_kw(p, Keyword::AFTER) {
            p.parse_identifier()?;
        }
    } else if opt_kw(p, Keyword::DROP) {
        p.expect_keyword(Keyword::COLUMN)?;
        if opt_kw(p, Keyword::IF) {
            p.expect_keyword(Keyword::EXISTS)?;
        }
        p.parse_object_name(true)?;
    } else if opt_kw(p, Keyword::ALTER) {
        p.expect_keyword(Keyword::COLUMN)?;
        p.parse_object_name(true)?;
        parse_alter_column_action(p)?;
    } else if opt_kw(p, Keyword::SET) {
        if consume_word(p, "properties") {
            if if_exists {
                return Err(ParserError::ParserError(
                    "ALTER TABLE SET PROPERTIES does not support IF EXISTS".into(),
                ));
            }
            parse_properties(p, false)?;
        } else if opt_kw(p, Keyword::AUTHORIZATION) {
            parse_principal(p)?;
        } else {
            return Err(ParserError::ParserError(
                "unsupported ALTER TABLE ... SET form".into(),
            ));
        }
    } else if opt_kw(p, Keyword::EXECUTE) {
        if if_exists {
            return Err(ParserError::ParserError(
                "ALTER TABLE EXECUTE does not support IF EXISTS".into(),
            ));
        }
        p.parse_identifier()?;
        parse_optional_execute_arguments(p)?;
        if opt_kw(p, Keyword::WHERE) {
            p.parse_expr()?;
        }
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER TABLE form".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_column_options(p: &mut Parser) -> Result<(), ParserError> {
    if consume_word(p, "default") {
        parse_trino_literal(p)?;
    }
    if opt_kw(p, Keyword::NOT) {
        p.expect_keyword(Keyword::NULL)?;
    }
    if opt_kw(p, Keyword::COMMENT) {
        parse_trino_string(p)?;
    }
    if opt_kw(p, Keyword::WITH) {
        parse_properties(p, true)?;
    }
    Ok(())
}

fn parse_alter_column_action(p: &mut Parser) -> Result<(), ParserError> {
    if opt_kw(p, Keyword::SET) {
        if opt_kw(p, Keyword::DATA) {
            p.expect_keyword(Keyword::TYPE)?;
            p.parse_data_type()?;
        } else if consume_word(p, "default") {
            parse_trino_literal(p)?;
        } else {
            return Err(ParserError::ParserError(
                "unsupported ALTER COLUMN ... SET form".into(),
            ));
        }
    } else if opt_kw(p, Keyword::DROP) {
        if !consume_word(p, "default") {
            p.expect_keyword(Keyword::NOT)?;
            p.expect_keyword(Keyword::NULL)?;
        }
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER COLUMN form".into(),
        ));
    }
    Ok(())
}

fn parse_trino_literal(p: &mut Parser) -> Result<(), ParserError> {
    let location = p.peek_token_ref().span.start;
    let expression = p.parse_expr()?;
    let is_literal = match expression {
        Expr::Value(_) | Expr::TypedString(_) | Expr::Interval(_) => true,
        Expr::UnaryOp {
            op: UnaryOperator::Plus | UnaryOperator::Minus,
            expr,
        } => matches!(*expr, Expr::Value(value) if matches!(value.value, Value::Number(_, _))),
        _ => false,
    };
    if !is_literal {
        return Err(ParserError::ParserError(format!(
            "Trino column DEFAULT requires a literal at Line: {}, Column: {}",
            location.line, location.column
        )));
    }
    Ok(())
}

fn parse_optional_execute_arguments(p: &mut Parser) -> Result<(), ParserError> {
    if !p.consume_token(&Token::LParen) {
        return Ok(());
    }
    if p.consume_token(&Token::RParen) {
        return Ok(());
    }
    loop {
        p.parse_function_args()?;
        if !p.consume_token(&Token::Comma) {
            break;
        }
    }
    p.expect_token(&Token::RParen)?;
    Ok(())
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
            parse_principal(p)?;
        } else {
            return Err(ParserError::ParserError(
                "unsupported ALTER MATERIALIZED VIEW ... SET form".into(),
            ));
        }
    } else if opt_kw(p, Keyword::EXECUTE) {
        p.parse_object_name(true)?;
        parse_optional_execute_arguments(p)?;
        if opt_kw(p, Keyword::WHERE) {
            p.parse_expr()?;
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
            parse_principal(p)?;
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
        match p.next_token().token {
            Token::Number(_, _) => {}
            other => {
                return Err(ParserError::ParserError(format!(
                    "ALTER BRANCH retention requires an integer, found {other}"
                )))
            }
        }
        consume_word(p, "day");
        consume_word(p, "days");
    } else {
        return Err(ParserError::ParserError(
            "unsupported ALTER BRANCH form".into(),
        ));
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn has_top_level_word_before(p: &Parser, expected: &str, boundary: &str) -> bool {
    let mut depth = 0usize;
    for index in 0.. {
        match p.peek_nth_token(index).token {
            Token::EOF | Token::SemiColon => return false,
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => depth = depth.saturating_sub(1),
            Token::Word(ref word) if depth == 0 => {
                if word.value.eq_ignore_ascii_case(expected) {
                    return true;
                }
                if word.value.eq_ignore_ascii_case(boundary) {
                    return false;
                }
            }
            _ => {}
        }
    }
    false
}

fn parse_identifier_list(p: &mut Parser) -> Result<(), ParserError> {
    loop {
        p.parse_identifier()?;
        if !p.consume_token(&Token::Comma) {
            return Ok(());
        }
    }
}

fn parse_principal_list(p: &mut Parser) -> Result<(), ParserError> {
    loop {
        parse_principal(p)?;
        if !p.consume_token(&Token::Comma) {
            return Ok(());
        }
    }
}

fn parse_privilege_list(p: &mut Parser) -> Result<(), ParserError> {
    if opt_kw(p, Keyword::ALL) {
        p.expect_keyword(Keyword::PRIVILEGES)?;
        return Ok(());
    }
    loop {
        let compound = is_unquoted_word_at(p, 0, "create") || is_unquoted_word_at(p, 0, "drop");
        p.parse_identifier()?;
        if compound {
            let _ = consume_word(p, "branch")
                || consume_word(p, "role")
                || consume_word(p, "table")
                || consume_word(p, "schema");
        } else if p.peek_keyword(Keyword::ROLE) {
            p.next_token();
        }
        if !p.consume_token(&Token::Comma) {
            return Ok(());
        }
    }
}

fn parse_grant_object(p: &mut Parser) -> Result<(), ParserError> {
    if consume_word(p, "branch") {
        p.parse_identifier()?;
        p.expect_keyword(Keyword::IN)?;
    }
    let known_kind = if opt_kw(p, Keyword::MATERIALIZED) {
        p.expect_keyword(Keyword::VIEW)?;
        true
    } else {
        opt_kw(p, Keyword::TABLE)
            || opt_kw(p, Keyword::SCHEMA)
            || consume_word(p, "function")
            || consume_word(p, "procedure")
    };
    if !known_kind
        && matches!(p.peek_token_ref().token, Token::Word(_))
        && matches!(p.peek_nth_token(1).token, Token::Word(_))
        && !is_unquoted_word_at(p, 1, "to")
        && !is_unquoted_word_at(p, 1, "from")
    {
        p.next_token();
    }
    p.parse_object_name(true)?;
    Ok(())
}

fn parse_optional_grantor_and_catalog(p: &mut Parser) -> Result<(), ParserError> {
    if opt_kw(p, Keyword::GRANTED) {
        p.expect_keyword(Keyword::BY)?;
        parse_grantor(p)?;
    }
    if opt_kw(p, Keyword::IN) {
        p.parse_identifier()?;
    }
    Ok(())
}

fn parse_grant(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::GRANT)?;
    if has_top_level_word_before(p, "on", "to") {
        parse_privilege_list(p)?;
        p.expect_keyword(Keyword::ON)?;
        parse_grant_object(p)?;
        p.expect_keyword(Keyword::TO)?;
        parse_principal(p)?;
        if opt_kw(p, Keyword::WITH) {
            p.expect_keyword(Keyword::GRANT)?;
            p.expect_keyword(Keyword::OPTION)?;
        }
    } else {
        parse_identifier_list(p)?;
        p.expect_keyword(Keyword::TO)?;
        parse_principal_list(p)?;
        if opt_kw(p, Keyword::WITH) {
            p.expect_keyword(Keyword::ADMIN)?;
            p.expect_keyword(Keyword::OPTION)?;
        }
        parse_optional_grantor_and_catalog(p)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_revoke(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::REVOKE)?;
    if has_top_level_word_before(p, "on", "from") {
        if opt_kw(p, Keyword::GRANT) {
            p.expect_keyword(Keyword::OPTION)?;
            p.expect_keyword(Keyword::FOR)?;
        }
        parse_privilege_list(p)?;
        p.expect_keyword(Keyword::ON)?;
        parse_grant_object(p)?;
        p.expect_keyword(Keyword::FROM)?;
        parse_principal(p)?;
    } else {
        if p.peek_keyword(Keyword::ADMIN) && is_unquoted_word_at(p, 1, "option") {
            p.next_token();
            p.expect_keyword(Keyword::OPTION)?;
            p.expect_keyword(Keyword::FOR)?;
        }
        parse_identifier_list(p)?;
        p.expect_keyword(Keyword::FROM)?;
        parse_principal_list(p)?;
        parse_optional_grantor_and_catalog(p)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_deny(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::DENY)?;
    parse_privilege_list(p)?;
    p.expect_keyword(Keyword::ON)?;
    parse_grant_object(p)?;
    p.expect_keyword(Keyword::TO)?;
    parse_principal(p)?;
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
            "ALTER TABLE t EXECUTE optimize (file_size_threshold => '16MB')",
            "ALTER TABLE IF EXISTS t RENAME TO t2",
            "ALTER TABLE t RENAME COLUMN IF EXISTS payload.old_name TO new_name",
            "ALTER TABLE t ADD COLUMN IF NOT EXISTS payload.item bigint LAST",
            "ALTER TABLE t ADD COLUMN payload.item varchar COMMENT 'item' WITH (x = 1) AFTER sibling",
            "ALTER TABLE t DROP COLUMN IF EXISTS payload.item",
            "ALTER TABLE t ALTER COLUMN payload.item SET DATA TYPE varchar",
            "ALTER TABLE t ALTER COLUMN payload.item SET DEFAULT 1",
            "ALTER TABLE t ALTER COLUMN payload.item DROP DEFAULT",
            "ALTER TABLE t ALTER COLUMN payload.item DROP NOT NULL",
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
            "CREATE ROLE role1",
            "CREATE ROLE role1 WITH ADMIN CURRENT_USER IN hive",
            "CREATE ROLE role1 WITH ADMIN ROLE admin IN hive",
            "DROP ROLE IF EXISTS role1 IN hive",
            "SET ROLE ALL IN hive",
            "SET ROLE NONE",
            "SET ROLE role1 IN hive",
            "GRANT role1, role2 TO USER u, ROLE r WITH ADMIN OPTION GRANTED BY CURRENT_ROLE IN hive",
            "REVOKE ADMIN OPTION FOR role1 FROM USER u GRANTED BY ROLE admin IN hive",
            "GRANT SELECT, INSERT ON TABLE hive.default.orders TO ROLE analyst WITH GRANT OPTION",
            "GRANT ALL PRIVILEGES ON BRANCH dev IN TABLE hive.default.orders TO USER alice",
            "REVOKE GRANT OPTION FOR SELECT ON SCHEMA hive.default FROM ROLE analyst",
            "DENY DELETE ON hive.default.orders TO USER alice",
            "CREATE FUNCTION testing.default.add_two(x bigint) RETURNS bigint COMMENT 'x' LANGUAGE SQL DETERMINISTIC RETURNS NULL ON NULL INPUT RETURN x + 2",
            "CREATE OR REPLACE FUNCTION f() RETURNS bigint RETURN 1",
            "CREATE FUNCTION external_f(x bigint) RETURNS bigint LANGUAGE python AS $$return x$$",
            "CREATE FUNCTION compound_f(n bigint) RETURNS bigint BEGIN DECLARE a bigint DEFAULT 1; SET a = a + n; RETURN a; END",
            "CREATE FUNCTION loop_f(n bigint) RETURNS bigint BEGIN WHILE n > 0 DO SET n = n - 1; END WHILE; RETURN n; END",
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
            "CREATE FUNCTION f() RETURNS bigint LANGUAGE SQL LANGUAGE SQL RETURN 1",
            "CREATE FUNCTION f() RETURNS bigint AS $tag$return 1$tag$",
            "CREATE FUNCTION f() RETURNS bigint BEGIN RETURN 1 END",
            "CREATE FUNCTION f() RETURNS bigint BEGIN IF true RETURN 1; END IF; END",
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
            "ALTER TABLE t EXECUTE system.optimize()",
            "ALTER TABLE IF EXISTS t EXECUTE optimize",
            "ALTER TABLE t RENAME COLUMN payload.item new_name",
            "ALTER TABLE t ADD COLUMN IF EXISTS payload.item bigint",
            "ALTER TABLE t ADD COLUMN payload.item bigint AFTER",
            "ALTER TABLE t DROP COLUMN IF NOT payload.item",
            "ALTER TABLE t ALTER COLUMN payload.item SET",
            "ALTER TABLE t ALTER COLUMN payload.item DROP NULL",
            "ALTER TABLE t SET AUTHORIZATION USER ROLE alice",
            "ALTER VIEW v SET AUTHORIZATION ROLE r trailing",
            "ALTER MATERIALIZED VIEW mv EXECUTE refresh (x = 1",
            "SET PATH one.too.many, qualifiers",
            "SET SESSION AUTHORIZATION null",
            "EXPLAIN VERBOSE SELECT * FROM t",
            "SHOW SESSION LIKE '%$_%' ESCAPE",
            "SHOW COLUMNS orders",
            "CREATE ROLE role1 WITH ADMIN",
            "CREATE ROLE role1 IN",
            "DROP ROLE role1 IN",
            "SET ROLE",
            "SET ROLE ALL trailing",
            "GRANT role1 TO USER u WITH GRANT OPTION",
            "GRANT SELECT ON TABLE t ROLE analyst",
            "GRANT ALL ON TABLE t TO ROLE analyst",
            "REVOKE ADMIN FOR role1 FROM USER u",
            "REVOKE SELECT ON TABLE t TO ROLE analyst",
            "DENY SELECT TABLE t TO ROLE analyst",
            "DENY SELECT ON TABLE t TO ROLE analyst trailing",
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
