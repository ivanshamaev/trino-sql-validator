from __future__ import annotations

import sys
from importlib import import_module
from pathlib import Path

import pytest
from trino_sql_validator import TypeWarning, validate

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
catalog_common = import_module("tools.catalog_common")
extract_functions = import_module("tools.extract_functions")
extract_types = import_module("tools.extract_types")


def test_type_extraction_adds_only_curated_geospatial_types() -> None:
    docs = {
        extract_types.TYPE_DOC: "### `VARCHAR`\n### `TIME(P) WITH TIME ZONE`\n",
        extract_types.GEOSPATIAL_DOC: ("Geometry SphericalGeography BingTile Point Polygon"),
    }

    names = extract_types.extract_names(docs)

    assert names == {
        "varchar",
        "time(p) with time zone",
        "time",
        "geometry",
        "sphericalgeography",
        "bingtile",
    }
    assert "point" not in names
    assert "polygon" not in names


def test_catalog_rendering_is_case_folded_sorted_and_deterministic() -> None:
    docs = {"one.md": ":::{function} Zeta\n{func}`alpha`\n:::{function} ALPHA"}

    names = extract_functions.extract_names(docs)
    first = extract_functions.render_rust(names)
    second = extract_functions.render_rust(names.copy())

    assert first == second
    assert first.index('"alpha"') < first.index('"zeta"')


def test_malformed_catalog_sources_fail_before_rendering() -> None:
    with pytest.raises(ValueError, match="no type headings"):
        extract_types.extract_names(
            {
                extract_types.TYPE_DOC: "no headings",
                extract_types.GEOSPATIAL_DOC: "Geometry SphericalGeography BingTile",
            }
        )
    with pytest.raises(ValueError, match="no documented function"):
        extract_functions.extract_names({"empty.md": "no declarations"})


def test_missing_required_local_source_is_an_error(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError, match="missing required documentation"):
        catalog_common.read_local_files(tmp_path, ["required.md"])


def test_check_mode_comparison_detects_drift(tmp_path: Path) -> None:
    output = tmp_path / "catalog.rs"
    output.write_text("old", encoding="utf-8")

    assert catalog_common.check_outputs({output: "new"}) is False
    assert output.read_text(encoding="utf-8") == "old"


@pytest.mark.parametrize("type_name", ["Geometry", "SphericalGeography", "BingTile"])
def test_documented_geospatial_types_do_not_warn(type_name: str) -> None:
    result = validate(f"SELECT CAST(NULL AS {type_name})")

    assert result.valid, result.error
    assert result.warnings == ()


def test_geospatial_type_typo_remains_advisory() -> None:
    sql = "SELECT CAST(NULL AS Geometery)"

    result = validate(sql)

    assert result.valid, result.error
    assert result.warnings == (TypeWarning("geometery", line=1, column=sql.index("Geometery") + 1),)
