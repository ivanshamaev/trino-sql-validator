from __future__ import annotations

import copy
import sys
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from audit_sqlglot import (
    SQLGLOT_VERSION,
    baseline_regressions,
    build_baseline,
    classify_sql,
    contains_command,
    ensure_sqlglot_version,
    stable_case_id,
)


def sample_report() -> dict[str, Any]:
    return {
        "metadata": {
            "sqlglot_version": SQLGLOT_VERSION,
            "trino": {"repository": "trinodb/trino", "revision": "abc"},
        },
        "sections": {
            "fixture_positive_prepared": {
                "total": 1,
                "state_counts": {"parsed_ast": 1},
                "cases": [
                    {
                        "id": stable_case_id("fixture_positive_prepared", "SELECT 1"),
                        "sql": "SELECT 1",
                        "state": "parsed_ast",
                    }
                ],
            }
        },
    }


def test_exact_sqlglot_version_is_enforced() -> None:
    ensure_sqlglot_version(SQLGLOT_VERSION)
    with pytest.raises(RuntimeError, match="expected exactly"):
        ensure_sqlglot_version("999.0.0")
    ensure_sqlglot_version("999.0.0", allow_mismatch=True)


def test_case_ids_and_baselines_are_deterministic() -> None:
    report = sample_report()

    assert stable_case_id("section", "SELECT 1") == stable_case_id("section", "SELECT 1")
    assert build_baseline(report) == build_baseline(copy.deepcopy(report))
    assert baseline_regressions(report, build_baseline(report)) == []


def test_baseline_detects_reduced_corpus_and_outcome_changes() -> None:
    report = sample_report()
    baseline = build_baseline(report)
    baseline["cases"]["fixture_positive_prepared"]["minimum_total"] = 2
    report["sections"]["fixture_positive_prepared"]["cases"][0]["state"] = "parse_error"
    report["sections"]["fixture_positive_prepared"]["state_counts"] = {"parse_error": 1}

    regressions = baseline_regressions(report, baseline)

    assert any("expected at least 2" in item for item in regressions)
    assert any("outcome changed" in item for item in regressions)
    assert any("state counts changed" in item for item in regressions)


def test_sqlglot_outcomes_distinguish_ast_command_and_errors() -> None:
    sqlglot = pytest.importorskip("sqlglot")
    from sqlglot import exp

    assert classify_sql("SELECT 1")["state"] == "parsed_ast"
    assert classify_sql("ALTER MATERIALIZED VIEW mv EXECUTE refresh")["state"] == (
        "command_fallback"
    )
    assert classify_sql("SELECT (")["state"] == "parse_error"
    assert classify_sql("SELECT 'unterminated")["state"] == "token_error"
    nested = sqlglot.parse_one("SELECT 1")
    nested.set("where", exp.Where(this=exp.Command(this="SHOW CATALOGS")))
    assert contains_command([nested]) is True
