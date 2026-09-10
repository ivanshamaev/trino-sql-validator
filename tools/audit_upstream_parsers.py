"""Compare the public validator with pinned Trino and PrestoDB parser tests."""

from __future__ import annotations

import argparse
import bisect
import json
import re
import sys
import urllib.parse
import urllib.request
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from trino_sql_validator import __version__, validate

TRINO_REPOSITORY = "trinodb/trino"
PRESTO_REPOSITORY = "prestodb/presto"

TRINO_TEST_FILES = {
    "parser": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParser.java",
    "errors": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParserErrorHandling.java",
    "functions": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParserFunctions.java",
    "routines": "core/trino-parser/src/test/java/io/trino/sql/parser/TestSqlParserRoutines.java",
    "types": "core/trino-parser/src/test/java/io/trino/sql/parser/TestTypeParser.java",
}

PRESTO_TEST_FILES = {
    "parser": "presto-parser/src/test/java/com/facebook/presto/sql/parser/TestSqlParser.java",
    "errors": "presto-parser/src/test/java/com/facebook/presto/sql/parser/TestSqlParserErrorHandling.java",
    "splitter": "presto-parser/src/test/java/com/facebook/presto/sql/parser/TestStatementSplitter.java",
}

METHOD_PATTERN = re.compile(r"\bvoid\s+(test[A-Za-z0-9_]+)\s*\(")
FORMAT_PATTERN = re.compile(r"%(?:s|d)")


@dataclass(frozen=True)
class Example:
    method: str
    sql: str


@dataclass(frozen=True)
class UpstreamSource:
    repository: str
    ref: str
    root: Path | None

    def read(self, relative_path: str) -> str:
        if self.root is not None:
            return (self.root / relative_path).read_text(encoding="utf-8")
        url = (
            f"https://raw.githubusercontent.com/{self.repository}/"
            f"{urllib.parse.quote(self.ref, safe='')}/{relative_path}"
        )
        with urllib.request.urlopen(url) as response:
            return response.read().decode("utf-8")

    def metadata(self) -> dict[str, str]:
        location = str(self.root.resolve()) if self.root is not None else "github"
        revision = self.ref
        if self.root is None:
            url = f"https://api.github.com/repos/{self.repository}/commits/{urllib.parse.quote(self.ref, safe='')}"
            request = urllib.request.Request(url, headers={"User-Agent": "trino-sql-validator-audit"})
            try:
                with urllib.request.urlopen(request) as response:
                    revision = json.load(response)["sha"]
            except (OSError, KeyError, json.JSONDecodeError):
                pass
        return {
            "repository": self.repository,
            "ref": self.ref,
            "revision": revision,
            "source": location,
        }


def decode_java_string(raw: str) -> str:
    try:
        return json.loads(f'"{raw}"')
    except json.JSONDecodeError:
        replacements = {
            r"\n": "\n",
            r"\r": "\r",
            r"\t": "\t",
            r'\"': '"',
            r"\\": "\\",
        }
        decoded = raw
        for old, new in replacements.items():
            decoded = decoded.replace(old, new)
        return decoded


def parse_string_sequence(source: str, position: int) -> tuple[str, int] | None:
    if position >= len(source) or source[position] != '"':
        return None
    parts: list[str] = []
    while position < len(source) and source[position] == '"':
        end = position + 1
        while end < len(source):
            if source[end] == "\\":
                end += 2
                continue
            if source[end] == '"':
                break
            end += 1
        if end >= len(source):
            return None
        parts.append(decode_java_string(source[position + 1 : end]))
        position = end + 1
        while position < len(source) and source[position].isspace():
            position += 1
        if position >= len(source) or source[position] != "+":
            break
        candidate = position + 1
        while candidate < len(source) and source[candidate].isspace():
            candidate += 1
        if candidate >= len(source) or source[candidate] != '"':
            break
        position = candidate
    return "".join(parts), position


def method_index(source: str) -> tuple[list[int], list[str]]:
    marks = [(match.start(), match.group(1)) for match in METHOD_PATTERN.finditer(source)]
    return [mark[0] for mark in marks], [mark[1] for mark in marks]


def method_at(position: int, positions: list[int], names: list[str]) -> str:
    index = bisect.bisect_right(positions, position) - 1
    return names[index] if index >= 0 else "outside_test_method"


def is_negative_wrapper(source: str, position: int) -> bool:
    prefix = source[max(0, position - 300) : position]
    return "assertThatThrownBy" in prefix[prefix.rfind(";") + 1 :]


def unique_examples(examples: Iterable[Example]) -> list[Example]:
    seen: set[str] = set()
    result: list[Example] = []
    for example in examples:
        if example.sql in seen:
            continue
        seen.add(example.sql)
        result.append(example)
    return result


def extract_call_examples(
    source: str,
    call_names: Iterable[str],
    *,
    exclude_negative_wrappers: bool = False,
) -> list[Example]:
    positions, names = method_index(source)
    examples: list[Example] = []
    for call_name in call_names:
        pattern = re.compile(rf"\b{re.escape(call_name)}\s*\(\s*")
        for match in pattern.finditer(source):
            if exclude_negative_wrappers and is_negative_wrapper(source, match.start()):
                continue
            parsed = parse_string_sequence(source, match.end())
            if parsed is None:
                continue
            sql, _ = parsed
            if FORMAT_PATTERN.search(sql):
                continue
            examples.append(Example(method_at(match.start(), positions, names), sql))
    return unique_examples(examples)


def between(source: str, start_marker: str, end_marker: str) -> str:
    start = source.index(start_marker)
    end = source.index(end_marker, start + len(start_marker))
    return source[start:end]


def extract_argument_examples(source: str, section: str) -> list[Example]:
    if section == "expressions":
        body = between(
            source,
            "private static Stream<Arguments> expressions()",
            "private static Stream<Arguments> statements()",
        )
    else:
        body = between(
            source,
            "private static Stream<Arguments> statements()",
            "    @Test",
        )
    examples: list[Example] = []
    pattern = re.compile(r"Arguments\.of\(\s*")
    for match in pattern.finditer(body):
        parsed = parse_string_sequence(body, match.end())
        if parsed is not None:
            examples.append(Example(section, parsed[0]))
    return unique_examples(examples)


def warning_data(result: Any) -> list[dict[str, Any]]:
    return [
        {
            "kind": type(warning).__name__,
            "name": warning.name,
            "line": warning.line,
            "column": warning.column,
        }
        for warning in result.warnings
    ]


def probe(
    examples: Iterable[Example],
    expected_valid: bool,
    wrapper: Callable[[str], str] = lambda sql: sql,
) -> dict[str, Any]:
    mismatches: list[dict[str, Any]] = []
    total = 0
    matched = 0
    for example in examples:
        total += 1
        sql = wrapper(example.sql)
        result = validate(sql)
        if result.valid is expected_valid:
            matched += 1
            continue
        mismatches.append(
            {
                "method": example.method,
                "sql": example.sql,
                "validated_sql": sql,
                "actual_valid": result.valid,
                "error": str(result.error) if result.error is not None else None,
                "warnings": warning_data(result),
            }
        )
    return {"total": total, "matched": matched, "mismatches": mismatches}


def file_inventory(files: dict[str, str], contents: dict[str, str]) -> dict[str, Any]:
    return {
        name: {
            "path": path,
            "lines": contents[name].count("\n") + 1,
            "test_methods": len(METHOD_PATTERN.findall(contents[name])),
        }
        for name, path in files.items()
    }


def run_audit(trino: UpstreamSource, presto: UpstreamSource) -> dict[str, Any]:
    trino_contents = {name: trino.read(path) for name, path in TRINO_TEST_FILES.items()}
    presto_contents = {name: presto.read(path) for name, path in PRESTO_TEST_FILES.items()}

    trino_statements = extract_call_examples(
        trino_contents["parser"],
        ("assertStatement", "statement"),
        exclude_negative_wrappers=True,
    )
    trino_expressions = extract_call_examples(
        trino_contents["parser"], ("expression",), exclude_negative_wrappers=True
    )
    trino_types = extract_call_examples(
        trino_contents["types"], ("type",), exclude_negative_wrappers=True
    )
    trino_direct_invalid = extract_call_examples(
        trino_contents["parser"], ("assertStatementIsInvalid",)
    )
    trino_error_statements = extract_argument_examples(trino_contents["errors"], "statements")
    trino_error_expressions = extract_argument_examples(
        trino_contents["errors"], "expressions"
    )
    presto_statements = extract_call_examples(
        presto_contents["parser"], ("assertStatement",), exclude_negative_wrappers=True
    )

    return {
        "validator_version": __version__,
        "limitations": [
            "Only ordinary Java string literals passed directly to known helpers are extracted.",
            "Java text blocks, variables, generated inputs and AST-only assertions are excluded.",
            "PrestoDB is a divergence check, not an acceptance target for dialect=trino.",
        ],
        "trino": {
            "source": trino.metadata(),
            "inventory": file_inventory(TRINO_TEST_FILES, trino_contents),
            "positive_statements": probe(trino_statements, True),
            "positive_expressions": probe(
                trino_expressions, True, lambda expression: f"SELECT {expression}"
            ),
            "positive_types": probe(
                trino_types, True, lambda data_type: f"SELECT CAST(NULL AS {data_type})"
            ),
            "negative_statements_direct": probe(trino_direct_invalid, False),
            "negative_statements_error_suite": probe(trino_error_statements, False),
            "negative_expressions_error_suite": probe(
                trino_error_expressions, False, lambda expression: f"SELECT {expression}"
            ),
        },
        "presto_comparison": {
            "source": presto.metadata(),
            "inventory": file_inventory(PRESTO_TEST_FILES, presto_contents),
            "positive_statements": probe(presto_statements, True),
        },
    }


def print_summary(report: dict[str, Any]) -> None:
    print(f"trino-sql-validator {report['validator_version']}")
    for label, data in (
        ("Trino", report["trino"]),
        ("PrestoDB comparison", report["presto_comparison"]),
    ):
        source = data["source"]
        print(
            f"\n{label}: {source['repository']} @ {source['ref']} "
            f"({source['revision'][:12]})"
        )
        for name, result in data.items():
            if not isinstance(result, dict) or "matched" not in result:
                continue
            expectation = "accepted" if name.startswith("positive") else "rejected"
            print(
                f"  {name}: {result['matched']}/{result['total']} {expectation}; "
                f"mismatches={len(result['mismatches'])}"
            )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trino-ref", default="483")
    parser.add_argument("--presto-ref", default="0.299")
    parser.add_argument("--trino-root", type=Path)
    parser.add_argument("--presto-root", type=Path)
    parser.add_argument("--format", choices=("summary", "json"), default="summary")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = run_audit(
        UpstreamSource(TRINO_REPOSITORY, args.trino_ref, args.trino_root),
        UpstreamSource(PRESTO_REPOSITORY, args.presto_ref, args.presto_root),
    )
    if args.format == "json":
        json.dump(report, sys.stdout, ensure_ascii=False, indent=2)
        print()
    else:
        print_summary(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
