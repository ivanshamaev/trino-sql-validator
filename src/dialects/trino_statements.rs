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
        Keyword::ALTER => p.maybe_parse(parse_alter).ok().flatten(),
        Keyword::CREATE => {
            // `CREATE FUNCTION` (Trino shape) is handled deterministically so a
            // missing `RETURNS`/`RETURN` reports an error instead of silently
            // falling through to sqlparser's (different) CREATE FUNCTION.
            if is_trino_create_function(p) {
                return Some(parse_create_function(p));
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
        Keyword::SET => p.maybe_parse(parse_set_path).ok().flatten(),
        Keyword::SHOW => p.maybe_parse(parse_show_create).ok().flatten(),
        _ => return None,
    };
    parsed.map(Ok)
}

/// `CREATE [OR REPLACE] [TEMP|TEMPORARY] FUNCTION <name> (`
fn is_trino_create_function(p: &mut Parser) -> bool {
    fn word_nth(p: &Parser, index: usize, word: &str) -> bool {
        matches!(
            p.peek_nth_token(index).token,
            Token::Word(w) if w.value.eq_ignore_ascii_case(word)
        )
    }
    let mut index = 1;
    if word_nth(p, index, "OR") {
        index += 1;
        if word_nth(p, index, "REPLACE") {
            index += 1;
        }
    }
    if word_nth(p, index, "TEMPORARY") || word_nth(p, index, "TEMP") {
        index += 1;
    }
    word_nth(p, index, "FUNCTION")
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

fn expect_word(p: &mut Parser, word: &str) -> Result<(), ParserError> {
    match p.peek_token_ref().token.clone() {
        Token::Word(w) if w.value.eq_ignore_ascii_case(word) => {
            p.next_token();
            Ok(())
        }
        other => Err(ParserError::ParserError(format!(
            "Expected {word}, found {other}"
        ))),
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

/// Parse `key = value [, ...]` property lists used by `ALTER TABLE/VIEW/
/// MATERIALIZED VIEW ... SET PROPERTIES` and `CREATE CATALOG`. Trino accepts
/// the list bare (`SET PROPERTIES x = 1`) or wrapped in `(...)`; at least one
/// property is required.
fn parse_properties(p: &mut Parser) -> Result<(), ParserError> {
    let parenthesized = p.consume_token(&Token::LParen);
    loop {
        p.parse_object_name(true)?;
        p.expect_token(&Token::Eq)?;
        // property value: any balanced run of tokens ending at a top-level
        // `,`, `)` or end of statement; at least one token is required
        let mut depth: i64 = 0;
        let mut consumed = 0;
        loop {
            match p.peek_token_ref().token.clone() {
                Token::EOF | Token::SemiColon => break,
                Token::Comma if depth == 0 => break,
                Token::RParen if depth == 0 => break,
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    consumed += 1;
                    p.next_token();
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    depth -= 1;
                    consumed += 1;
                    p.next_token();
                }
                _ => {
                    consumed += 1;
                    p.next_token();
                }
            }
        }
        if consumed == 0 {
            return Err(ParserError::ParserError(
                "property value must not be empty".into(),
            ));
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
    p.parse_object_name(true)?;
    while p.consume_token(&Token::Comma) {
        p.parse_object_name(true)?;
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
    if opt_kw(p, Keyword::OR) {
        p.expect_keyword(Keyword::REPLACE)?;
    }
    if consume_word(p, "branch") {
        return parse_create_branch(p);
    }
    if opt_kw(p, Keyword::CATALOG) {
        return parse_create_catalog(p);
    }
    Err(ParserError::ParserError(
        "not a Trino-only CREATE statement".into(),
    ))
}

fn parse_create_catalog(p: &mut Parser) -> Result<Statement, ParserError> {
    p.parse_object_name(true)?;
    p.expect_keyword(Keyword::USING)?;
    p.parse_object_name(true)?;
    if p.peek_token_ref().token == Token::LParen {
        parse_properties(p)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
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

fn parse_create_branch(p: &mut Parser) -> Result<Statement, ParserError> {
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::NOT)?;
        p.expect_keyword(Keyword::EXISTS)?;
    }
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::IN) {
        p.expect_keyword(Keyword::TABLE)?;
        p.parse_object_name(true)?;
    }
    if opt_kw(p, Keyword::FROM) {
        p.parse_object_name(true)?;
    } else if consume_word(p, "as") {
        expect_word(p, "of")?;
        consume_to_end(p)?;
    }
    end_of_statement(p)?;
    Ok(placeholder())
}

fn parse_drop(p: &mut Parser) -> Result<Statement, ParserError> {
    p.expect_keyword(Keyword::DROP)?;
    let mut is_branch = false;
    if opt_kw(p, Keyword::CATALOG) {
        // DROP CATALOG name
    } else if consume_word(p, "branch") {
        is_branch = true;
    } else {
        return Err(ParserError::ParserError(
            "not a Trino-only DROP statement".into(),
        ));
    }
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
    }
    p.parse_object_name(true)?;
    if is_branch && opt_kw(p, Keyword::IN) {
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
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
    }
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::SET) {
        if consume_word(p, "properties") {
            parse_properties(p)?;
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
    if opt_kw(p, Keyword::IF) {
        p.expect_keyword(Keyword::EXISTS)?;
    }
    p.parse_object_name(true)?;
    if opt_kw(p, Keyword::RENAME) {
        p.expect_keyword(Keyword::TO)?;
        p.parse_object_name(true)?;
    } else if opt_kw(p, Keyword::SET) {
        if consume_word(p, "properties") {
            parse_properties(p)?;
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
        } else if consume_word(p, "properties") {
            parse_properties(p)?;
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
            "DESCRIBE INPUT stmt",
            "DESCRIBE OUTPUT my_query",
            "DESCRIBE OUTPUT my_query WHERE output = 1",
            "REFRESH MATERIALIZED VIEW mv",
            "CREATE CATALOG hive USING hive",
            "CREATE CATALOG hive USING hive (hive.metastore.uri = 'thrift://host:9083')",
            "CREATE OR REPLACE CATALOG h2 USING hive",
            "DROP CATALOG hive",
            "CREATE BRANCH b1 IN TABLE t",
            "CREATE OR REPLACE BRANCH b2 IN TABLE t FROM b1",
            "DROP BRANCH b1 IN TABLE t",
            "ALTER BRANCH b1 IN TABLE t FAST FORWARD TO t2",
            "ALTER BRANCH b2 SET RETENTION 3 DAYS",
            "ALTER TABLE t SET PROPERTIES (x = 1)",
            "ALTER TABLE t SET PROPERTIES x = 1",
            "ALTER TABLE t SET PROPERTIES (x = 'v', loc = 's3://b')",
            "ALTER TABLE t SET PROPERTIES x = 'v', loc = 's3://b'",
            "ALTER TABLE t SET AUTHORIZATION ROLE role1",
            "ALTER TABLE t SET AUTHORIZATION USER user1",
            "ALTER TABLE t EXECUTE optimize",
            "ALTER TABLE t EXECUTE optimize (file_size_threshold = '16MB')",
            "ALTER VIEW v RENAME TO v2",
            "ALTER VIEW v REFRESH",
            "ALTER VIEW v SET AUTHORIZATION ROLE r",
            "ALTER VIEW v SET PROPERTIES (p = 'q')",
            "ALTER MATERIALIZED VIEW mv RENAME TO mv2",
            "ALTER MATERIALIZED VIEW mv SET PROPERTIES (p = 'q')",
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
            "DROP BRANCH",
            "CREATE FUNCTION f()",
            "CREATE FUNCTION f() RETURNS bigint COMMENT 'x'",
            "ALTER TABLE t SET PROPERTIES",
            "ALTER TABLE t SET PROPERTIES ()",
            "ALTER TABLE t SET PROPERTIES (x = )",
            "ALTER TABLE t SET PROPERTIES x = ",
            "ALTER BRANCH b SET",
            "ALTER TABLE t EXECUTE",
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
