use std::any::TypeId;

use sqlparser::ast::{Expr, GranteesType, Statement};
use sqlparser::dialect::{Dialect, GenericDialect, Precedence};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};

use super::{trino_statements, TrinoDialect};

/// [`sqlparser::dialect::Dialect`] implementation for Trino.
///
/// Trino-specific lexing and statement parsing are overridden below; every
/// other trait hook is delegated to `GenericDialect`. Generated from
/// sqlparser's trait with `tools/gen_generic_delegates.py`; regenerate it when
/// bumping the sqlparser version.
impl Dialect for TrinoDialect {
    fn is_delimited_identifier_start(&self, ch: char) -> bool {
        ch == '"'
    }

    fn is_identifier_start(&self, ch: char) -> bool {
        ch.is_alphabetic() || ch == '_'
    }

    fn is_identifier_part(&self, ch: char) -> bool {
        ch.is_alphabetic() || ch.is_ascii_digit() || ch == '_' || ch == '$'
    }

    fn supports_string_literal_backslash_escape(&self) -> bool {
        false
    }

    fn parse_statement(&self, parser: &mut Parser) -> Option<Result<Statement, ParserError>> {
        trino_statements::try_parse_statement(parser)
    }

    fn allow_extract_custom(&self) -> bool {
        GenericDialect::allow_extract_custom(&GenericDialect {})
    }

    fn allow_extract_single_quotes(&self) -> bool {
        GenericDialect::allow_extract_single_quotes(&GenericDialect {})
    }

    fn convert_type_before_value(&self) -> bool {
        GenericDialect::convert_type_before_value(&GenericDialect {})
    }

    fn describe_requires_table_keyword(&self) -> bool {
        GenericDialect::describe_requires_table_keyword(&GenericDialect {})
    }

    fn dialect(&self) -> TypeId {
        GenericDialect::dialect(&GenericDialect {})
    }

    fn get_next_precedence_default(&self, parser: &Parser) -> Result<u8, ParserError> {
        GenericDialect::get_next_precedence_default(&GenericDialect {}, parser)
    }

    fn get_next_precedence(&self, parser: &Parser) -> Option<Result<u8, ParserError>> {
        GenericDialect::get_next_precedence(&GenericDialect {}, parser)
    }

    fn get_reserved_grantees_types(&self) -> &[GranteesType] {
        GenericDialect::get_reserved_grantees_types(&GenericDialect {})
    }

    fn get_reserved_keywords_for_select_item_operator(&self) -> &[Keyword] {
        GenericDialect::get_reserved_keywords_for_select_item_operator(&GenericDialect {})
    }

    fn identifier_quote_style(&self, identifier: &str) -> Option<char> {
        GenericDialect::identifier_quote_style(&GenericDialect {}, identifier)
    }

    fn ignores_wildcard_escapes(&self) -> bool {
        GenericDialect::ignores_wildcard_escapes(&GenericDialect {})
    }

    fn is_column_alias(&self, kw: &Keyword, parser: &mut Parser) -> bool {
        GenericDialect::is_column_alias(&GenericDialect {}, kw, parser)
    }

    fn is_custom_operator_part(&self, ch: char) -> bool {
        GenericDialect::is_custom_operator_part(&GenericDialect {}, ch)
    }

    fn is_nested_delimited_identifier_start(&self, ch: char) -> bool {
        GenericDialect::is_nested_delimited_identifier_start(&GenericDialect {}, ch)
    }

    fn is_reserved_for_identifier(&self, kw: Keyword) -> bool {
        if kw == Keyword::TOP {
            return false;
        }
        GenericDialect::is_reserved_for_identifier(&GenericDialect {}, kw)
    }

    fn is_select_item_alias(&self, explicit: bool, kw: &Keyword, parser: &mut Parser) -> bool {
        GenericDialect::is_select_item_alias(&GenericDialect {}, explicit, kw, parser)
    }

    fn is_table_alias(&self, kw: &Keyword, parser: &mut Parser) -> bool {
        if *kw == Keyword::TOP {
            return true;
        }
        GenericDialect::is_table_alias(&GenericDialect {}, kw, parser)
    }

    fn is_table_factor_alias(&self, explicit: bool, kw: &Keyword, parser: &mut Parser) -> bool {
        if *kw == Keyword::TOP {
            return true;
        }
        GenericDialect::is_table_factor_alias(&GenericDialect {}, explicit, kw, parser)
    }

    fn is_table_factor(&self, kw: &Keyword, parser: &mut Parser) -> bool {
        GenericDialect::is_table_factor(&GenericDialect {}, kw, parser)
    }

    fn parse_prefix(&self, parser: &mut Parser) -> Option<Result<Expr, ParserError>> {
        GenericDialect::parse_prefix(&GenericDialect {}, parser)
    }

    fn prec_unknown(&self) -> u8 {
        GenericDialect::prec_unknown(&GenericDialect {})
    }

    fn prec_value(&self, prec: Precedence) -> u8 {
        GenericDialect::prec_value(&GenericDialect {}, prec)
    }

    fn require_interval_qualifier(&self) -> bool {
        GenericDialect::require_interval_qualifier(&GenericDialect {})
    }

    fn requires_single_line_comment_whitespace(&self) -> bool {
        GenericDialect::requires_single_line_comment_whitespace(&GenericDialect {})
    }

    fn support_map_literal_syntax(&self) -> bool {
        GenericDialect::support_map_literal_syntax(&GenericDialect {})
    }

    fn supports_alter_column_type_using(&self) -> bool {
        GenericDialect::supports_alter_column_type_using(&GenericDialect {})
    }

    fn supports_array_join_syntax(&self) -> bool {
        GenericDialect::supports_array_join_syntax(&GenericDialect {})
    }

    fn supports_array_typedef_with_brackets(&self) -> bool {
        GenericDialect::supports_array_typedef_with_brackets(&GenericDialect {})
    }

    fn supports_array_typedef_without_element_type(&self) -> bool {
        GenericDialect::supports_array_typedef_without_element_type(&GenericDialect {})
    }

    fn supports_asc_desc_in_column_definition(&self) -> bool {
        GenericDialect::supports_asc_desc_in_column_definition(&GenericDialect {})
    }

    fn supports_bang_not_operator(&self) -> bool {
        GenericDialect::supports_bang_not_operator(&GenericDialect {})
    }

    fn supports_binary_kw_as_cast(&self) -> bool {
        GenericDialect::supports_binary_kw_as_cast(&GenericDialect {})
    }

    fn supports_bitwise_shift_operators(&self) -> bool {
        GenericDialect::supports_bitwise_shift_operators(&GenericDialect {})
    }

    fn supports_boolean_literals(&self) -> bool {
        GenericDialect::supports_boolean_literals(&GenericDialect {})
    }

    fn supports_column_definition_trailing_commas(&self) -> bool {
        GenericDialect::supports_column_definition_trailing_commas(&GenericDialect {})
    }

    fn supports_comma_separated_drop_column_list(&self) -> bool {
        GenericDialect::supports_comma_separated_drop_column_list(&GenericDialect {})
    }

    fn supports_comma_separated_set_assignments(&self) -> bool {
        GenericDialect::supports_comma_separated_set_assignments(&GenericDialect {})
    }

    fn supports_comma_separated_trim(&self) -> bool {
        GenericDialect::supports_comma_separated_trim(&GenericDialect {})
    }

    fn supports_comment_on(&self) -> bool {
        GenericDialect::supports_comment_on(&GenericDialect {})
    }

    fn supports_comment_optimizer_hint(&self) -> bool {
        GenericDialect::supports_comment_optimizer_hint(&GenericDialect {})
    }

    fn supports_connect_by(&self) -> bool {
        GenericDialect::supports_connect_by(&GenericDialect {})
    }

    fn supports_constraint_keyword_without_name(&self) -> bool {
        GenericDialect::supports_constraint_keyword_without_name(&GenericDialect {})
    }

    fn supports_create_index_with_clause(&self) -> bool {
        GenericDialect::supports_create_index_with_clause(&GenericDialect {})
    }

    fn supports_create_table_like_parenthesized(&self) -> bool {
        GenericDialect::supports_create_table_like_parenthesized(&GenericDialect {})
    }

    fn supports_create_table_multi_schema_info_sources(&self) -> bool {
        GenericDialect::supports_create_table_multi_schema_info_sources(&GenericDialect {})
    }

    fn supports_create_table_select(&self) -> bool {
        GenericDialect::supports_create_table_select(&GenericDialect {})
    }

    fn supports_create_table_using(&self) -> bool {
        GenericDialect::supports_create_table_using(&GenericDialect {})
    }

    fn supports_create_view_comment_syntax(&self) -> bool {
        GenericDialect::supports_create_view_comment_syntax(&GenericDialect {})
    }

    fn supports_cross_join_constraint(&self) -> bool {
        GenericDialect::supports_cross_join_constraint(&GenericDialect {})
    }

    fn supports_cte_without_as(&self) -> bool {
        GenericDialect::supports_cte_without_as(&GenericDialect {})
    }

    fn supports_data_type_signed_suffix(&self) -> bool {
        GenericDialect::supports_data_type_signed_suffix(&GenericDialect {})
    }

    fn supports_detach(&self) -> bool {
        GenericDialect::supports_detach(&GenericDialect {})
    }

    fn supports_dictionary_syntax(&self) -> bool {
        GenericDialect::supports_dictionary_syntax(&GenericDialect {})
    }

    fn supports_dollar_as_money_prefix(&self) -> bool {
        GenericDialect::supports_dollar_as_money_prefix(&GenericDialect {})
    }

    fn supports_dollar_placeholder(&self) -> bool {
        GenericDialect::supports_dollar_placeholder(&GenericDialect {})
    }

    fn supports_double_ampersand_operator(&self) -> bool {
        GenericDialect::supports_double_ampersand_operator(&GenericDialect {})
    }

    fn supports_empty_projections(&self) -> bool {
        GenericDialect::supports_empty_projections(&GenericDialect {})
    }

    fn supports_end_transaction_modifier(&self) -> bool {
        GenericDialect::supports_end_transaction_modifier(&GenericDialect {})
    }

    fn supports_eq_alias_assignment(&self) -> bool {
        GenericDialect::supports_eq_alias_assignment(&GenericDialect {})
    }

    fn supports_execute_immediate(&self) -> bool {
        GenericDialect::supports_execute_immediate(&GenericDialect {})
    }

    fn supports_explain_with_utility_options(&self) -> bool {
        GenericDialect::supports_explain_with_utility_options(&GenericDialect {})
    }

    fn supports_extract_comma_syntax(&self) -> bool {
        GenericDialect::supports_extract_comma_syntax(&GenericDialect {})
    }

    fn supports_factorial_operator(&self) -> bool {
        GenericDialect::supports_factorial_operator(&GenericDialect {})
    }

    fn supports_filter_during_aggregation(&self) -> bool {
        GenericDialect::supports_filter_during_aggregation(&GenericDialect {})
    }

    fn supports_from_first_insert(&self) -> bool {
        GenericDialect::supports_from_first_insert(&GenericDialect {})
    }

    fn supports_from_first_select(&self) -> bool {
        GenericDialect::supports_from_first_select(&GenericDialect {})
    }

    fn supports_from_trailing_commas(&self) -> bool {
        GenericDialect::supports_from_trailing_commas(&GenericDialect {})
    }

    fn supports_geometric_types(&self) -> bool {
        GenericDialect::supports_geometric_types(&GenericDialect {})
    }

    fn supports_group_by_expr(&self) -> bool {
        GenericDialect::supports_group_by_expr(&GenericDialect {})
    }

    fn supports_group_by_with_modifier(&self) -> bool {
        GenericDialect::supports_group_by_with_modifier(&GenericDialect {})
    }

    fn supports_in_empty_list(&self) -> bool {
        GenericDialect::supports_in_empty_list(&GenericDialect {})
    }

    fn supports_insert_format(&self) -> bool {
        GenericDialect::supports_insert_format(&GenericDialect {})
    }

    fn supports_insert_set(&self) -> bool {
        GenericDialect::supports_insert_set(&GenericDialect {})
    }

    fn supports_insert_table_alias(&self) -> bool {
        GenericDialect::supports_insert_table_alias(&GenericDialect {})
    }

    fn supports_insert_table_function(&self) -> bool {
        GenericDialect::supports_insert_table_function(&GenericDialect {})
    }

    fn supports_insert_table_query(&self) -> bool {
        GenericDialect::supports_insert_table_query(&GenericDialect {})
    }

    fn supports_install(&self) -> bool {
        GenericDialect::supports_install(&GenericDialect {})
    }

    fn supports_interpolate(&self) -> bool {
        GenericDialect::supports_interpolate(&GenericDialect {})
    }

    fn supports_interval_options(&self) -> bool {
        GenericDialect::supports_interval_options(&GenericDialect {})
    }

    fn supports_key_column_option(&self) -> bool {
        GenericDialect::supports_key_column_option(&GenericDialect {})
    }

    fn supports_lambda_functions(&self) -> bool {
        GenericDialect::supports_lambda_functions(&GenericDialect {})
    }

    fn supports_left_associative_joins_without_parens(&self) -> bool {
        GenericDialect::supports_left_associative_joins_without_parens(&GenericDialect {})
    }

    fn supports_limit_by(&self) -> bool {
        GenericDialect::supports_limit_by(&GenericDialect {})
    }

    fn supports_limit_comma(&self) -> bool {
        GenericDialect::supports_limit_comma(&GenericDialect {})
    }

    fn supports_listen_notify(&self) -> bool {
        GenericDialect::supports_listen_notify(&GenericDialect {})
    }

    fn supports_load_data(&self) -> bool {
        GenericDialect::supports_load_data(&GenericDialect {})
    }

    fn supports_load_extension(&self) -> bool {
        GenericDialect::supports_load_extension(&GenericDialect {})
    }

    fn supports_long_type_as_bigint(&self) -> bool {
        GenericDialect::supports_long_type_as_bigint(&GenericDialect {})
    }

    fn supports_map_literal_with_angle_brackets(&self) -> bool {
        GenericDialect::supports_map_literal_with_angle_brackets(&GenericDialect {})
    }

    fn supports_match_against(&self) -> bool {
        GenericDialect::supports_match_against(&GenericDialect {})
    }

    fn supports_match_recognize(&self) -> bool {
        GenericDialect::supports_match_recognize(&GenericDialect {})
    }

    fn supports_multiline_comment_hints(&self) -> bool {
        GenericDialect::supports_multiline_comment_hints(&GenericDialect {})
    }

    fn supports_named_fn_args_with_assignment_operator(&self) -> bool {
        GenericDialect::supports_named_fn_args_with_assignment_operator(&GenericDialect {})
    }

    fn supports_named_fn_args_with_colon_operator(&self) -> bool {
        GenericDialect::supports_named_fn_args_with_colon_operator(&GenericDialect {})
    }

    fn supports_named_fn_args_with_eq_operator(&self) -> bool {
        GenericDialect::supports_named_fn_args_with_eq_operator(&GenericDialect {})
    }

    fn supports_named_fn_args_with_expr_name(&self) -> bool {
        GenericDialect::supports_named_fn_args_with_expr_name(&GenericDialect {})
    }

    fn supports_named_fn_args_with_rarrow_operator(&self) -> bool {
        GenericDialect::supports_named_fn_args_with_rarrow_operator(&GenericDialect {})
    }

    fn supports_nested_comments(&self) -> bool {
        GenericDialect::supports_nested_comments(&GenericDialect {})
    }

    fn supports_notnull_operator(&self) -> bool {
        GenericDialect::supports_notnull_operator(&GenericDialect {})
    }

    fn supports_numeric_literal_underscores(&self) -> bool {
        GenericDialect::supports_numeric_literal_underscores(&GenericDialect {})
    }

    fn supports_numeric_prefix(&self) -> bool {
        GenericDialect::supports_numeric_prefix(&GenericDialect {})
    }

    fn supports_object_name_double_dot_notation(&self) -> bool {
        GenericDialect::supports_object_name_double_dot_notation(&GenericDialect {})
    }

    fn supports_optimize_table(&self) -> bool {
        GenericDialect::supports_optimize_table(&GenericDialect {})
    }

    fn supports_order_by_all(&self) -> bool {
        GenericDialect::supports_order_by_all(&GenericDialect {})
    }

    fn supports_outer_join_operator(&self) -> bool {
        GenericDialect::supports_outer_join_operator(&GenericDialect {})
    }

    fn supports_parens_around_table_factor(&self) -> bool {
        GenericDialect::supports_parens_around_table_factor(&GenericDialect {})
    }

    fn supports_parenthesized_set_variables(&self) -> bool {
        GenericDialect::supports_parenthesized_set_variables(&GenericDialect {})
    }

    fn supports_partiql(&self) -> bool {
        GenericDialect::supports_partiql(&GenericDialect {})
    }

    fn supports_partition_by_after_order_by(&self) -> bool {
        GenericDialect::supports_partition_by_after_order_by(&GenericDialect {})
    }

    fn supports_pipe_operator(&self) -> bool {
        GenericDialect::supports_pipe_operator(&GenericDialect {})
    }

    fn supports_prewhere(&self) -> bool {
        GenericDialect::supports_prewhere(&GenericDialect {})
    }

    fn supports_projection_trailing_commas(&self) -> bool {
        GenericDialect::supports_projection_trailing_commas(&GenericDialect {})
    }

    fn supports_quote_delimited_string(&self) -> bool {
        GenericDialect::supports_quote_delimited_string(&GenericDialect {})
    }

    fn supports_select_exclude(&self) -> bool {
        GenericDialect::supports_select_exclude(&GenericDialect {})
    }

    fn supports_select_expr_star(&self) -> bool {
        GenericDialect::supports_select_expr_star(&GenericDialect {})
    }

    fn supports_select_format(&self) -> bool {
        GenericDialect::supports_select_format(&GenericDialect {})
    }

    fn supports_select_item_multi_column_alias(&self) -> bool {
        GenericDialect::supports_select_item_multi_column_alias(&GenericDialect {})
    }

    fn supports_select_modifiers(&self) -> bool {
        GenericDialect::supports_select_modifiers(&GenericDialect {})
    }

    fn supports_select_wildcard_except(&self) -> bool {
        GenericDialect::supports_select_wildcard_except(&GenericDialect {})
    }

    fn supports_select_wildcard_exclude(&self) -> bool {
        GenericDialect::supports_select_wildcard_exclude(&GenericDialect {})
    }

    fn supports_select_wildcard_ilike(&self) -> bool {
        GenericDialect::supports_select_wildcard_ilike(&GenericDialect {})
    }

    fn supports_select_wildcard_rename(&self) -> bool {
        GenericDialect::supports_select_wildcard_rename(&GenericDialect {})
    }

    fn supports_select_wildcard_replace(&self) -> bool {
        GenericDialect::supports_select_wildcard_replace(&GenericDialect {})
    }

    fn supports_select_wildcard_with_alias(&self) -> bool {
        GenericDialect::supports_select_wildcard_with_alias(&GenericDialect {})
    }

    fn supports_semantic_view_table_factor(&self) -> bool {
        GenericDialect::supports_semantic_view_table_factor(&GenericDialect {})
    }

    fn supports_set_names(&self) -> bool {
        GenericDialect::supports_set_names(&GenericDialect {})
    }

    fn supports_set_stmt_without_operator(&self) -> bool {
        GenericDialect::supports_set_stmt_without_operator(&GenericDialect {})
    }

    fn supports_settings(&self) -> bool {
        GenericDialect::supports_settings(&GenericDialect {})
    }

    fn supports_show_like_before_in(&self) -> bool {
        GenericDialect::supports_show_like_before_in(&GenericDialect {})
    }

    fn supports_space_separated_column_options(&self) -> bool {
        GenericDialect::supports_space_separated_column_options(&GenericDialect {})
    }

    fn supports_start_transaction_modifier(&self) -> bool {
        GenericDialect::supports_start_transaction_modifier(&GenericDialect {})
    }

    fn supports_string_escape_constant(&self) -> bool {
        GenericDialect::supports_string_escape_constant(&GenericDialect {})
    }

    fn supports_string_literal_concatenation(&self) -> bool {
        GenericDialect::supports_string_literal_concatenation(&GenericDialect {})
    }

    fn supports_string_literal_concatenation_with_newline(&self) -> bool {
        GenericDialect::supports_string_literal_concatenation_with_newline(&GenericDialect {})
    }

    fn supports_struct_literal(&self) -> bool {
        GenericDialect::supports_struct_literal(&GenericDialect {})
    }

    fn supports_subquery_as_function_arg(&self) -> bool {
        GenericDialect::supports_subquery_as_function_arg(&GenericDialect {})
    }

    fn supports_table_hints(&self) -> bool {
        GenericDialect::supports_table_hints(&GenericDialect {})
    }

    fn supports_table_sample_before_alias(&self) -> bool {
        GenericDialect::supports_table_sample_before_alias(&GenericDialect {})
    }

    fn supports_table_versioning(&self) -> bool {
        GenericDialect::supports_table_versioning(&GenericDialect {})
    }

    fn supports_top_before_distinct(&self) -> bool {
        GenericDialect::supports_top_before_distinct(&GenericDialect {})
    }

    fn supports_trailing_commas(&self) -> bool {
        GenericDialect::supports_trailing_commas(&GenericDialect {})
    }

    fn supports_triple_quoted_string(&self) -> bool {
        GenericDialect::supports_triple_quoted_string(&GenericDialect {})
    }

    fn supports_try_convert(&self) -> bool {
        GenericDialect::supports_try_convert(&GenericDialect {})
    }

    fn supports_unicode_string_literal(&self) -> bool {
        GenericDialect::supports_unicode_string_literal(&GenericDialect {})
    }

    fn supports_update_order_by(&self) -> bool {
        GenericDialect::supports_update_order_by(&GenericDialect {})
    }

    fn supports_user_host_grantee(&self) -> bool {
        GenericDialect::supports_user_host_grantee(&GenericDialect {})
    }

    fn supports_values_as_table_factor(&self) -> bool {
        GenericDialect::supports_values_as_table_factor(&GenericDialect {})
    }

    fn supports_window_clause_named_window_reference(&self) -> bool {
        GenericDialect::supports_window_clause_named_window_reference(&GenericDialect {})
    }

    fn supports_window_function_null_treatment_arg(&self) -> bool {
        GenericDialect::supports_window_function_null_treatment_arg(&GenericDialect {})
    }

    fn supports_with_fill(&self) -> bool {
        GenericDialect::supports_with_fill(&GenericDialect {})
    }

    fn supports_within_after_array_aggregation(&self) -> bool {
        GenericDialect::supports_within_after_array_aggregation(&GenericDialect {})
    }

    fn supports_xml_expressions(&self) -> bool {
        GenericDialect::supports_xml_expressions(&GenericDialect {})
    }
}
