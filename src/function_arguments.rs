use core::ops::ControlFlow;
use std::collections::HashSet;

use sqlparser::ast::{
    visit_expressions, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Statement,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionArgumentIssue {
    pub(crate) name: String,
    pub(crate) actual_count: usize,
    pub(crate) expected_counts: Vec<usize>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
}

impl FunctionArgumentIssue {
    pub(crate) fn message(&self) -> String {
        let argument = if self.expected_counts == [1] {
            "1 argument".to_string()
        } else {
            let expected = self
                .expected_counts
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(" or ");
            format!("{expected} arguments")
        };
        format!(
            "Trino built-in function '{}' expects {}; got {}",
            self.name, argument, self.actual_count
        )
    }
}

fn expected_argument_counts(name: &str) -> Option<&'static [usize]> {
    match name {
        "any_value" | "approx_set" | "arbitrary" | "array_agg" | "avg" | "bitwise_and_agg"
        | "bitwise_or_agg" | "bitwise_xor_agg" | "bool_and" | "bool_or" | "checksum"
        | "count_if" | "every" | "geometric_mean" | "histogram" | "kurtosis" | "map_union"
        | "merge" | "skewness" | "stddev" | "stddev_pop" | "stddev_samp" | "sum" | "variance"
        | "var_pop" | "var_samp" => Some(&[1]),
        "corr" | "covar_pop" | "covar_samp" | "map_agg" | "multimap_agg" | "regr_intercept"
        | "regr_slope" => Some(&[2]),
        "approx_most_frequent" => Some(&[3]),
        "reduce_agg" => Some(&[4]),
        "count" => Some(&[0, 1]),
        "approx_distinct" | "max" | "min" | "tdigest_agg" => Some(&[1, 2]),
        "approx_percentile" | "max_by" | "min_by" | "numeric_histogram" => Some(&[2, 3]),
        "qdigest_agg" => Some(&[1, 2, 3]),
        _ => None,
    }
}

pub(crate) fn find_function_argument_issues(
    statements: &[Statement],
    local_function_names: &HashSet<String>,
) -> Vec<FunctionArgumentIssue> {
    let mut issues = Vec::new();
    let owned_statements = statements.to_vec();
    let _ = visit_expressions(&owned_statements, |expr| {
        let Expr::Function(function) = expr else {
            return ControlFlow::<()>::Continue(());
        };
        if function.name.0.len() != 1 {
            return ControlFlow::Continue(());
        }
        let Some(ident) = function.name.0[0].as_ident() else {
            return ControlFlow::Continue(());
        };
        if ident.quote_style.is_some() {
            return ControlFlow::Continue(());
        }
        let name = ident.value.to_ascii_lowercase();
        let Some(expected_counts) = expected_argument_counts(&name) else {
            return ControlFlow::Continue(());
        };
        if local_function_names.contains(&name) {
            return ControlFlow::Continue(());
        }
        let FunctionArguments::List(arguments) = &function.args else {
            return ControlFlow::Continue(());
        };
        if arguments
            .args
            .iter()
            .any(|argument| !matches!(argument, FunctionArg::Unnamed(FunctionArgExpr::Expr(_))))
        {
            return ControlFlow::Continue(());
        }
        let actual_count = arguments.args.len();
        if !expected_counts.contains(&actual_count) {
            issues.push(FunctionArgumentIssue {
                name,
                actual_count,
                expected_counts: expected_counts.to_vec(),
                line: Some(ident.span.start.line as usize),
                column: Some(ident.span.start.column as usize),
            });
        }
        ControlFlow::Continue(())
    });
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::dialect::GenericDialect;
    use sqlparser::parser::Parser;

    #[test]
    fn finds_invalid_documented_aggregate_counts() {
        let statements = Parser::parse_sql(
            &GenericDialect {},
            concat!(
                "SELECT sum(), min(), max(1, 2, 3), max_by(1), avg(1, 2), ",
                "map_agg(1), approx_percentile(1), reduce_agg(1, 2, 3), count(1, 2)"
            ),
        )
        .unwrap();

        let issues = find_function_argument_issues(&statements, &HashSet::new());

        assert_eq!(issues.len(), 9);
        assert_eq!(issues[0].expected_counts, [1]);
        assert_eq!(issues[1].expected_counts, [1, 2]);
        assert_eq!(issues[3].expected_counts, [2, 3]);
        assert_eq!(issues[7].expected_counts, [4]);
        assert_eq!(issues[8].expected_counts, [0, 1]);
    }

    #[test]
    fn accepts_documented_aggregate_counts_and_excluded_forms() {
        let statements = Parser::parse_sql(
            &GenericDialect {},
            concat!(
                "SELECT sum(x), min(x), min(x, 2), max(x), max(x, 2), ",
                "max_by(x, y), max_by(x, y, 2), approx_distinct(x), ",
                "approx_distinct(x, 0.01), approx_percentile(x, 0.5), ",
                "approx_percentile(x, w, 0.5), qdigest_agg(x, w, 0.01), ",
                "reduce_agg(x, 0, f, g), count(), count(x), count(*), listagg(x), ",
                "schema.sum(), sum(*), custom_agg()"
            ),
        )
        .unwrap();

        assert!(find_function_argument_issues(&statements, &HashSet::new()).is_empty());
    }

    #[test]
    fn respects_inline_function_scope() {
        let statements = Parser::parse_sql(&GenericDialect {}, "SELECT sum()").unwrap();
        let local_names = HashSet::from(["sum".to_string()]);

        assert!(find_function_argument_issues(&statements, &local_names).is_empty());
    }
}
