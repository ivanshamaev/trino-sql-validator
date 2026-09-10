use pyo3::exceptions::PyValueError;
use pyo3::PyErr;

use sqlparser::dialect::{Dialect, GenericDialect, HiveDialect};

pub mod trino_statements;

mod generic_delegates;
mod trino_types;

pub(crate) use trino_types::parse_sql as parse_trino_sql;

/// SQL dialects supported by the validator.
///
/// `Trino` is the focus; `Hive` and `Generic` are offered as permissive
/// alternates when a query uses syntax that the Trino override does not yet
/// cover. See `plan/roadmap.md` for the long-term plan for Trino fidelity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlDialect {
    Trino,
    Hive,
    Generic,
}

impl std::str::FromStr for SqlDialect {
    type Err = PyErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "trino" => Ok(SqlDialect::Trino),
            "hive" => Ok(SqlDialect::Hive),
            "generic" => Ok(SqlDialect::Generic),
            other => Err(PyValueError::new_err(format!(
                "unknown dialect '{}'; expected one of: trino, hive, generic",
                other
            ))),
        }
    }
}

impl SqlDialect {
    /// Build the concrete `sqlparser` dialect for this enum value.
    pub fn parser(&self) -> Box<dyn Dialect> {
        match self {
            SqlDialect::Trino => Box::new(TrinoDialect {}),
            SqlDialect::Hive => Box::new(HiveDialect {}),
            SqlDialect::Generic => Box::new(GenericDialect {}),
        }
    }
}

/// A `sqlparser` dialect tuned for Trino (Presto-family) syntax.
///
/// Trino has no dedicated upstream dialect, so this is a thin override on top
/// of [`GenericDialect`]. Behaviour matches `GenericDialect` for every trait
/// hook (see [`dialects::generic_delegates`]) except for the Trino-specific
/// lexing rules and statement parsing ([`trino_statements`]). The most
/// impactful lexing difference today: Trino rejects backquoted identifiers (it
/// allows only double-quoted identifiers), whereas Generic/Hive accept
/// backticks. The actual [`Dialect`] impl lives in `generic_delegates.rs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TrinoDialect;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn dialect_names_resolve_case_insensitively() {
        assert_eq!(SqlDialect::from_str("Trino").unwrap(), SqlDialect::Trino);
        assert_eq!(SqlDialect::from_str("HIVE").unwrap(), SqlDialect::Hive);
        assert!(SqlDialect::from_str("postgres").is_err());
    }

    #[test]
    fn trino_rejects_backquote_identifier() {
        let trino = SqlDialect::Trino.parser();
        let sql = "SELECT `foo` FROM t";
        assert!(sqlparser::parser::Parser::parse_sql(trino.as_ref(), sql).is_err());
    }

    #[test]
    fn trino_accepts_double_quote_identifier() {
        let trino = SqlDialect::Trino.parser();
        let sql = "SELECT \"foo\" FROM t";
        assert!(sqlparser::parser::Parser::parse_sql(trino.as_ref(), sql).is_ok());
    }

    #[test]
    fn generic_accepts_backquote_identifier() {
        let generic = SqlDialect::Generic.parser();
        let sql = "SELECT `foo` FROM t";
        assert!(sqlparser::parser::Parser::parse_sql(generic.as_ref(), sql).is_ok());
    }
}
