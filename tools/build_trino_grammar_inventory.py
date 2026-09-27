"""Build the dev-only Trino grammar rule-to-regression inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any

RULE_PATTERN = re.compile(
    r"^([a-z][A-Za-z0-9_]*)(?:\[[^\n]*\])?(?:(?:\s+)|(?://[^\n]*\n))*:",
    re.MULTILINE,
)
LABEL_PATTERN = re.compile(r"#([A-Za-z][A-Za-z0-9_]*)")
ENTRY_POINT_RULES = {
    "singleStatement",
    "standaloneExpression",
    "standalonePathSpecification",
    "standaloneType",
    "standaloneRowPattern",
    "standaloneFunctionSpecification",
}
COMPONENT_PATHS = {
    "grammar": "core/trino-grammar/src/main/antlr4/io/trino/grammar/sql/SqlBase.g4",
    "sql_parser": "core/trino-parser/src/main/java/io/trino/sql/parser/SqlParser.java",
    "post_processor": "core/trino-parser/src/main/java/io/trino/sql/parser/SqlParser.java",
    "ast_builder": "core/trino-parser/src/main/java/io/trino/sql/parser/AstBuilder.java",
}


def _scan_rule_end(source: str, start: int) -> int:
    round_depth = square_depth = brace_depth = 0
    quote: str | None = None
    line_comment = block_comment = False
    index = start
    while index < len(source):
        char = source[index]
        following = source[index + 1] if index + 1 < len(source) else ""
        if line_comment:
            line_comment = char != "\n"
        elif block_comment:
            if char == "*" and following == "/":
                block_comment = False
                index += 1
        elif quote is not None:
            if char == "\\":
                index += 1
            elif char == quote:
                quote = None
        elif char == "/" and following == "/":
            line_comment = True
            index += 1
        elif char == "/" and following == "*":
            block_comment = True
            index += 1
        elif char in {"'", '"'}:
            quote = char
        elif char == "(":
            round_depth += 1
        elif char == ")":
            round_depth -= 1
        elif char == "[":
            square_depth += 1
        elif char == "]":
            square_depth -= 1
        elif char == "{":
            brace_depth += 1
        elif char == "}":
            brace_depth -= 1
        elif char == ";" and round_depth == square_depth == brace_depth == 0:
            return index
        index += 1
    raise ValueError("unterminated grammar rule")


def _split_alternatives(body: str) -> list[str]:
    alternatives = []
    start = 0
    round_depth = square_depth = brace_depth = 0
    quote: str | None = None
    line_comment = block_comment = False
    index = 0
    while index < len(body):
        char = body[index]
        following = body[index + 1] if index + 1 < len(body) else ""
        if line_comment:
            line_comment = char != "\n"
        elif block_comment:
            if char == "*" and following == "/":
                block_comment = False
                index += 1
        elif quote is not None:
            if char == "\\":
                index += 1
            elif char == quote:
                quote = None
        elif char == "/" and following == "/":
            line_comment = True
            index += 1
        elif char == "/" and following == "*":
            block_comment = True
            index += 1
        elif char in {"'", '"'}:
            quote = char
        elif char == "(":
            round_depth += 1
        elif char == ")":
            round_depth -= 1
        elif char == "[":
            square_depth += 1
        elif char == "]":
            square_depth -= 1
        elif char == "{":
            brace_depth += 1
        elif char == "}":
            brace_depth -= 1
        elif char == "|" and round_depth == square_depth == brace_depth == 0:
            alternatives.append(body[start:index].strip())
            start = index + 1
        index += 1
    alternatives.append(body[start:].strip())
    return alternatives


def parsed_grammar_rules(source: str) -> list[dict[str, Any]]:
    rules = []
    for match in RULE_PATTERN.finditer(source):
        end = _scan_rule_end(source, match.end())
        alternatives = []
        for index, text in enumerate(_split_alternatives(source[match.end() : end]), 1):
            labels = LABEL_PATTERN.findall(text)
            label = labels[-1] if labels else None
            alternatives.append(
                {
                    "index": index,
                    "id": label or f"alt-{index}",
                    "label": label,
                    "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
                }
            )
        rules.append({"rule": match.group(1), "alternatives": alternatives})
    return rules


def grammar_rules(source: str) -> list[tuple[str, list[str]]]:
    return [
        (
            row["rule"],
            [item["label"] for item in row["alternatives"] if item["label"]],
        )
        for row in parsed_grammar_rules(source)
    ]


def component_metadata(trino_root: Path) -> dict[str, Any]:
    result = {}
    for name, relative in COMPONENT_PATHS.items():
        path = trino_root / relative
        if not path.is_file():
            raise FileNotFoundError(path)
        result[name] = {
            "path": relative,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        if name == "post_processor":
            result[name]["symbol"] = "SqlParser.PostProcessor"
    return result


def build_inventory(
    grammar: str,
    corpus: dict[str, Any],
    components: dict[str, Any] | None = None,
) -> dict[str, Any]:
    references: dict[str, set[str]] = {}
    detailed_references: dict[str, dict[str, set[str]]] = {}
    for case in corpus["cases"]:
        for reference in case["grammar_refs"]:
            rule, _, alternative = reference.partition("/")
            references.setdefault(rule, set()).add(case["id"])
            detailed_references.setdefault(rule, {}).setdefault(alternative, set()).add(case["id"])
    rows = []
    for parsed in parsed_grammar_rules(grammar):
        rule = parsed["rule"]
        case_ids = sorted(references.get(rule, set()))
        if case_ids:
            status = "partial"
            note = "Selected alternatives/interactions have executable regression cases."
        elif rule in ENTRY_POINT_RULES:
            status = "entry-point-only"
            note = "Covered indirectly through the corresponding parser entry point."
        else:
            status = "missing"
            note = "No case in the focused v0.20 grammar corpus claims this rule."
        alternative_rows = []
        matched_references = set()
        for alternative in parsed["alternatives"]:
            label = alternative["label"]
            alternative_case_ids = sorted(detailed_references.get(rule, {}).get(label or "", set()))
            if label and alternative_case_ids:
                matched_references.add(label)
            alternative_rows.append(
                {
                    **alternative,
                    "status": "partial" if alternative_case_ids else "missing",
                    "case_ids": alternative_case_ids,
                    "pytest_ids": [
                        "tests/test_v020_trino_grammar_cases.py::"
                        f"test_trino_grammar_case[{case_id}]"
                        for case_id in alternative_case_ids
                    ],
                }
            )
        named_references = {
            value for value in detailed_references.get(rule, {}) if value
        }
        rows.append(
            {
                "rule": rule,
                "alternative_count": len(alternative_rows),
                "alternatives": alternative_rows,
                "status": status,
                "case_ids": case_ids,
                "pytest_ids": [
                    "tests/test_v020_trino_grammar_cases.py::"
                    f"test_trino_grammar_case[{case_id}]"
                    for case_id in case_ids
                ],
                "coverage_references": {
                    reference or "rule-level": sorted(ids)
                    for reference, ids in sorted(detailed_references.get(rule, {}).items())
                },
                "unmatched_alternative_references": sorted(
                    named_references - matched_references
                ),
                "note": note,
            }
        )
    counts: dict[str, int] = {}
    for row in rows:
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    alternative_counts: dict[str, int] = {}
    for row in rows:
        for alternative in row["alternatives"]:
            status = alternative["status"]
            alternative_counts[status] = alternative_counts.get(status, 0) + 1
    corpus_bytes = json.dumps(corpus, sort_keys=True, ensure_ascii=False).encode("utf-8")
    return {
        "schema_version": 1,
        "trino_ref": "483",
        "trino_revision": corpus["source"]["revision"],
        "grammar_path": corpus["source"]["grammar_path"],
        "grammar_sha256": hashlib.sha256(grammar.encode("utf-8")).hexdigest(),
        "corpus_sha256": hashlib.sha256(corpus_bytes).hexdigest(),
        "upstream_components": components or {},
        "implementation_layers": {
            "grammar": {"status": "partial", "component": "grammar"},
            "lexer": {
                "status": "partial",
                "components": ["grammar", "post_processor"],
            },
            "post_processor": {"status": "partial", "component": "post_processor"},
            "ast_builder": {"status": "partial", "component": "ast_builder"},
        },
        "status_meaning": {
            "partial": "At least one selected branch or interaction is covered; full rule coverage is not claimed.",
            "entry-point-only": "The entry rule is exercised through wrapped cases, not mapped as a syntax feature.",
            "missing": "No focused v0.20 case maps to the rule; it remains visible backlog.",
        },
        "summary": {
            "rules": len(rows),
            "rule_statuses": counts,
            "alternatives": sum(len(row["alternatives"]) for row in rows),
            "alternative_statuses": alternative_counts,
        },
        "rules": rows,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trino-root", type=Path, required=True)
    parser.add_argument("--grammar", type=Path)
    parser.add_argument(
        "--corpus", type=Path, default=Path("tests/cases/trino_483_grammar.json")
    )
    parser.add_argument(
        "--output", type=Path, default=Path("plan/trino_483_grammar_inventory.json")
    )
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    grammar_path = args.grammar or args.trino_root / COMPONENT_PATHS["grammar"]
    grammar = grammar_path.read_text(encoding="utf-8")
    corpus = json.loads(args.corpus.read_text(encoding="utf-8"))
    rendered = (
        json.dumps(
            build_inventory(grammar, corpus, component_metadata(args.trino_root)),
            ensure_ascii=False,
            indent=2,
        )
        + "\n"
    )
    if args.check:
        if not args.output.is_file() or args.output.read_text(encoding="utf-8") != rendered:
            raise SystemExit(f"grammar inventory is stale: {args.output}")
        return 0
    args.output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
