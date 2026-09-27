from __future__ import annotations

import json
import sys
from importlib import import_module
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
inventory_tool = import_module("tools.build_trino_grammar_inventory")

CORPUS = json.loads(
    (ROOT / "tests/cases/trino_483_grammar.json").read_text(encoding="utf-8")
)
INVENTORY = json.loads(
    (ROOT / "plan/trino_483_grammar_inventory.json").read_text(encoding="utf-8")
)


def test_inventory_parser_handles_arguments_and_comments_before_colon() -> None:
    grammar = """
first
    : ONE
    ;
predicate[ParserRuleContext value]
    : TWO #second
    ;
nonReserved
    // a comment between the rule name and colon
    : THREE
    ;
"""

    assert inventory_tool.grammar_rules(grammar) == [
        ("first", []),
        ("predicate", ["second"]),
        ("nonReserved", []),
    ]


def test_inventory_parser_splits_only_top_level_grammar_alternatives() -> None:
    grammar = r"""
rule
    : ('a' | 'b') #nested
    | ONE { helper("|;"); } // comment with | and ;
      TWO #action
    | THREE
    ;
"""

    parsed = inventory_tool.parsed_grammar_rules(grammar)

    assert len(parsed) == 1
    assert [alternative["id"] for alternative in parsed[0]["alternatives"]] == [
        "nested",
        "action",
        "alt-3",
    ]


def test_committed_trino_grammar_inventory_is_complete_and_consistent() -> None:
    rows = INVENTORY["rules"]
    rule_names = {row["rule"] for row in rows}
    case_ids = {case["id"] for case in CORPUS["cases"]}

    assert INVENTORY["trino_ref"] == "483"
    assert INVENTORY["grammar_sha256"] == CORPUS["source"]["grammar_sha256"]
    assert INVENTORY["summary"]["rules"] == len(rows) == 144
    assert INVENTORY["summary"]["alternatives"] == 708
    assert sum(row["alternative_count"] for row in rows) == 708
    assert len(rule_names) == len(rows)
    assert INVENTORY["summary"]["rule_statuses"] == {
        "entry-point-only": 6,
        "missing": 90,
        "partial": 48,
    }
    assert INVENTORY["summary"]["alternative_statuses"] == {
        "missing": 387,
        "partial": 321,
    }
    assert {row["status"] for row in rows} <= {
        "partial",
        "entry-point-only",
        "missing",
        "covered",
        "out-of-scope",
    }
    assert all(set(row["case_ids"]) <= case_ids for row in rows)
    assert all(not row["unmatched_alternative_references"] for row in rows)
    assert set(INVENTORY["upstream_components"]) == {
        "grammar",
        "sql_parser",
        "post_processor",
        "ast_builder",
    }
    assert all(
        len(component["sha256"]) == 64
        for component in INVENTORY["upstream_components"].values()
    )
    assert set(INVENTORY["implementation_layers"]) == {
        "grammar",
        "lexer",
        "post_processor",
        "ast_builder",
    }
    assert all(
        alternative["status"] in {"partial", "missing"}
        and len(alternative["sha256"]) == 64
        and all(case_id in case_ids for case_id in alternative["case_ids"])
        for row in rows
        for alternative in row["alternatives"]
    )
    assert all(
        reference.split("/", 1)[0] in rule_names
        for case in CORPUS["cases"]
        for reference in case["grammar_refs"]
    )
