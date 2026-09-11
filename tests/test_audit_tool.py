from __future__ import annotations

import hashlib
import sys
from importlib import import_module
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
audit = import_module("tools.audit_upstream_parsers")


def test_java_text_blocks_are_extracted_without_empty_artifacts() -> None:
    source = '''
        void testTextBlock() {
            statement(
                    """
                    SELECT 1
                    """);
            statement(sqlVariable);
            statement("");
        }
    '''

    extraction = audit.extract_call_examples(source, ("statement",), source_file="Test.java")

    assert [example.sql.strip() for example in extraction.examples] == ["SELECT 1"]
    assert extraction.examples[0].method == "testTextBlock"
    assert extraction.examples[0].source_file == "Test.java"
    assert extraction.examples[0].line is not None
    assert extraction.skipped["non_literal"] == 1
    assert extraction.skipped["empty_sql_api_difference"] == 1
    assert extraction.malformed == []


def test_unterminated_java_text_block_is_malformed_extraction() -> None:
    extraction = audit.extract_call_examples(
        'void testBroken() { statement(""" SELECT 1); }', ("statement",)
    )

    assert extraction.examples == []
    assert len(extraction.malformed) == 1


def test_standalone_function_specification_has_an_executable_wrapper() -> None:
    extraction = audit.extract_call_examples(
        'void testFunction() { functionSpecification("FUNCTION f() RETURNS bigint RETURN 1"); }',
        ("functionSpecification",),
    )

    result = audit.probe(extraction.examples, True, lambda sql: f"CREATE {sql}")

    assert result["matched"] == 1
    assert result["mismatches"] == []


def test_local_source_metadata_uses_actual_git_revision_and_hash() -> None:
    source = audit.UpstreamSource("trinodb/trino", "ignored-local-ref", ROOT)

    metadata = source.metadata({"parser": "SELECT 1"})

    assert len(metadata["revision"]) == 40
    assert metadata["revision"] != "ignored-local-ref"
    assert metadata["dirty"] is True
    assert metadata["content_sha256"]["parser"] == hashlib.sha256(b"SELECT 1").hexdigest()


def test_baseline_gate_detects_new_mismatch_and_missing_cases() -> None:
    report = {
        "trino": {
            "source": {"repository": "trinodb/trino", "revision": "abc"},
            "positive_statements": {
                "total": 1,
                "mismatches": [{"validated_sql": "SELECT broken"}],
            },
            "extraction": {},
        }
    }
    baseline = {
        "repository": "trinodb/trino",
        "revision": "abc",
        "cases": {
            "positive_statements": {
                "minimum_total": 2,
                "allowed_mismatches": [],
            }
        },
    }

    regressions = audit.baseline_regressions(report, baseline)

    assert any("expected at least 2" in regression for regression in regressions)
    assert any("new mismatch" in regression for regression in regressions)
