from __future__ import annotations

import base64
import json
import subprocess
import sys
from importlib import import_module
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
oracle = import_module("tools.run_trino_parser_oracle")
corpus_builder = import_module("tools.build_trino_oracle_corpus")


def test_oracle_input_requires_versioned_unique_cases(tmp_path: Path) -> None:
    path = tmp_path / "cases.json"
    path.write_text(
        json.dumps(
            {
                "schema_version": 2,
                "cases": [
                    {"id": "one", "entry_point": "createStatement", "sql": "SELECT 1"},
                    {"id": "one", "entry_point": "createExpression", "sql": "1"},
                ],
            }
        ),
        encoding="utf-8",
    )

    with pytest.raises(ValueError, match="unique"):
        oracle.load_cases(path)


def test_oracle_protocol_keeps_rejection_separate_from_infrastructure_error() -> None:
    cases = [
        {"id": "ok", "entry_point": "createStatement", "sql": "SELECT 1"},
        {"id": "bad", "entry_point": "createStatement", "sql": "SELECT FROM"},
        {"id": "infra", "entry_point": "createType", "sql": "BIGINT"},
    ]
    message = lambda value: base64.b64encode(value.encode()).decode()
    output = "\n".join(
        [
            "ok\tACCEPTED",
            f"bad\tREJECTED\tio.trino.sql.parser.ParsingException\t1\t8\t{message('mismatched input')}",
            f"infra\tINFRASTRUCTURE_ERROR\tjava.lang.LinkageError\t{message('linkage failure')}",
        ]
    )

    outcomes = oracle.parse_output(output, cases)

    assert [item["status"] for item in outcomes] == [
        "accepted",
        "rejected",
        "infrastructure_error",
    ]
    assert outcomes[1]["line"] == 1
    assert outcomes[1]["column"] == 8
    assert outcomes[1]["error_class"] == "io.trino.sql.parser.ParsingException"
    assert outcomes[2]["error_class"] == "java.lang.LinkageError"
    assert all(len(item["input_sha256"]) == 64 for item in outcomes)


def test_oracle_protocol_rejects_incomplete_output() -> None:
    cases = [{"id": "one", "entry_point": "createStatement", "sql": "SELECT 1"}]

    with pytest.raises(RuntimeError, match="omitted"):
        oracle.parse_output("", cases)


def test_project_oracle_corpus_contains_every_required_offline_section() -> None:
    cases = corpus_builder.build_cases()
    counts: dict[str, int] = {}
    for case in cases:
        counts[case["source"]] = counts.get(case["source"], 0) + 1

    assert counts == {
        "positive_fixture": 553,
        "independent_negative_fixture": 236,
        "diagnostic_datamart": 30,
        "grammar_regression": 100,
        "composition_matrix": 84,
        "transformation_matrix": 28,
        "mutation_regression": 6,
        "compatibility_exception": 3,
    }
    assert len({case["id"] for case in cases}) == len(cases) == 1040
    assert all(case["expected_status"] in {"accepted", "rejected"} for case in cases)
    assert all("raw_sql" in case and "preparation" in case for case in cases)
    assert all(
        case["compatibility_reason"]
        for case in cases
        if case["source"] == "compatibility_exception"
    )


def _fake_oracle_dir(tmp_path: Path) -> tuple[Path, Path]:
    oracle_dir = tmp_path / "oracle"
    classes = oracle_dir / "target/classes/dev/example"
    classes.mkdir(parents=True)
    (classes / "Oracle.class").write_bytes(b"class")
    dependency = tmp_path / "trino-parser.jar"
    dependency.write_bytes(b"jar")
    (oracle_dir / "target/classpath.txt").write_text(str(dependency), encoding="utf-8")
    source = oracle_dir / "src/main/java/dev/trino_sql_validator/TrinoParserOracle.java"
    source.parent.mkdir(parents=True)
    source.write_text("final class Oracle {}", encoding="utf-8")
    (oracle_dir / "pom.xml").write_text("<project/>", encoding="utf-8")
    return oracle_dir, dependency


def test_oracle_report_records_input_and_tool_provenance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    oracle_dir, dependency = _fake_oracle_dir(tmp_path)
    cases = oracle.load_cases(
        _write_cases(
            tmp_path,
            [
                {
                    "id": "one",
                    "entry_point": "createStatement",
                    "sql": "SELECT 1",
                    "raw_sql": "SELECT {{ value }}",
                    "preparation": "jinja_auto",
                    "expected_status": "accepted",
                }
            ],
        )
    )

    def fake_run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        if command[1:] == ["-version"]:
            return subprocess.CompletedProcess(command, 0, "", "openjdk 23")
        return subprocess.CompletedProcess(command, 0, "one\tACCEPTED\n", "")

    monkeypatch.setattr(oracle.subprocess, "run", fake_run)

    report = oracle.run_oracle(cases, oracle_dir, "java", 1)

    assert report["complete"] is True
    assert report["case_count"] == 1
    assert report["outcomes"][0]["input_sha256"] == cases[0]["input_sha256"]
    assert report["outcomes"][0]["raw_input_sha256"] == cases[0]["raw_input_sha256"]
    provenance = report["oracle_provenance"]
    assert provenance["java_version"] == "openjdk 23"
    assert provenance["classpath_artifacts"][0]["name"] == dependency.name
    assert len(provenance["classpath_artifacts"][0]["sha256"]) == 64
    assert len(provenance["source_sha256"]) == 64
    assert len(provenance["pom_sha256"]) == 64


def _write_cases(tmp_path: Path, cases: list[dict[str, object]]) -> Path:
    path = tmp_path / "oracle-cases.json"
    path.write_text(json.dumps({"schema_version": 2, "cases": cases}), encoding="utf-8")
    return path


def test_oracle_timeout_is_an_infrastructure_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    oracle_dir, _ = _fake_oracle_dir(tmp_path)
    cases = oracle.load_cases(
        _write_cases(
            tmp_path,
            [{"id": "one", "entry_point": "createStatement", "sql": "SELECT 1"}],
        )
    )

    def fake_run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        if command[1:] == ["-version"]:
            return subprocess.CompletedProcess(command, 0, "", "openjdk 23")
        raise subprocess.TimeoutExpired(command, 1)

    monkeypatch.setattr(oracle.subprocess, "run", fake_run)

    with pytest.raises(subprocess.TimeoutExpired):
        oracle.run_oracle(cases, oracle_dir, "java", 1)


def test_oracle_nonzero_exit_is_not_a_sql_rejection(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    oracle_dir, _ = _fake_oracle_dir(tmp_path)
    cases = oracle.load_cases(
        _write_cases(
            tmp_path,
            [{"id": "one", "entry_point": "createStatement", "sql": "SELECT 1"}],
        )
    )

    def fake_run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        if command[1:] == ["-version"]:
            return subprocess.CompletedProcess(command, 0, "", "openjdk 23")
        return subprocess.CompletedProcess(command, 9, "", "crash")

    monkeypatch.setattr(oracle.subprocess, "run", fake_run)

    with pytest.raises(RuntimeError, match="exited with 9"):
        oracle.run_oracle(cases, oracle_dir, "java", 1)
