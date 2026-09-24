use sqlparser::keywords::Keyword;

/// Trino 483 reserved identifiers from `docs/src/main/sphinx/language/reserved.md`.
pub(crate) const RESERVED_KEYWORDS: [&str; 83] = [
    "ALTER",
    "AND",
    "AS",
    "AUTO",
    "BETWEEN",
    "BY",
    "CASE",
    "CAST",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CUBE",
    "CURRENT_CATALOG",
    "CURRENT_DATE",
    "CURRENT_PATH",
    "CURRENT_ROLE",
    "CURRENT_SCHEMA",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "DEALLOCATE",
    "DELETE",
    "DESCRIBE",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "ESCAPE",
    "EXCEPT",
    "EXISTS",
    "EXTRACT",
    "FALSE",
    "FOR",
    "FROM",
    "FULL",
    "GROUP",
    "GROUPING",
    "HAVING",
    "IN",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "JSON_ARRAY",
    "JSON_EXISTS",
    "JSON_OBJECT",
    "JSON_QUERY",
    "JSON_TABLE",
    "JSON_VALUE",
    "LEFT",
    "LIKE",
    "LISTAGG",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "NATURAL",
    "NORMALIZE",
    "NOT",
    "NULL",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVERLAPS",
    "PREPARE",
    "RECURSIVE",
    "RIGHT",
    "ROLLUP",
    "SELECT",
    "SKIP",
    "TABLE",
    "THEN",
    "TRIM",
    "TRUE",
    "UESCAPE",
    "UNION",
    "UNNEST",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITH",
];

pub(crate) fn is_reserved_word(value: &str) -> bool {
    RESERVED_KEYWORDS
        .iter()
        .any(|keyword| value.eq_ignore_ascii_case(keyword))
}

pub(crate) fn is_reserved_keyword(keyword: Keyword) -> bool {
    keyword != Keyword::NoKeyword && is_reserved_word(&keyword.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_keyword_catalog_is_sorted_and_unique() {
        assert_eq!(RESERVED_KEYWORDS.len(), 83);
        assert!(RESERVED_KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn reserved_keyword_lookup_is_case_insensitive() {
        assert!(is_reserved_word("where"));
        assert!(is_reserved_keyword(Keyword::WHERE));
        assert!(!is_reserved_word("partition"));
        assert!(!is_reserved_keyword(Keyword::PARTITION));
    }
}
