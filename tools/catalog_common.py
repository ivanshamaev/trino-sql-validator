from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

GITHUB_API = "https://api.github.com/repos/trinodb/trino"
RAW_BASE = "https://raw.githubusercontent.com/trinodb/trino"


def fetch_bytes(url: str) -> bytes:
    request = urllib.request.Request(
        url,
        headers={"Accept": "application/vnd.github+json", "User-Agent": "trino-sql-validator"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def resolve_ref(ref: str) -> str:
    payload = json.loads(fetch_bytes(f"{GITHUB_API}/commits/{ref}").decode("utf-8"))
    sha = payload.get("sha")
    if not isinstance(sha, str) or not sha:
        raise ValueError(f"GitHub returned no commit SHA for ref {ref!r}")
    return sha


def read_remote_files(ref: str, paths: list[str]) -> tuple[dict[str, str], dict[str, Any]]:
    sha = resolve_ref(ref)
    docs: dict[str, str] = {}
    for path in paths:
        docs[path] = fetch_bytes(f"{RAW_BASE}/{sha}/{path}").decode("utf-8")
    return docs, source_manifest("remote", ref, sha, docs)


def git_metadata(root: Path) -> tuple[str, bool]:
    sha = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    dirty = bool(
        subprocess.run(
            ["git", "-C", str(root), "status", "--porcelain"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    return sha, dirty


def read_local_files(root: Path, paths: list[str]) -> tuple[dict[str, str], dict[str, Any]]:
    docs: dict[str, str] = {}
    missing = []
    for relative in paths:
        path = root / relative
        if not path.is_file():
            missing.append(relative)
            continue
        docs[relative] = path.read_text(encoding="utf-8")
    if missing:
        raise FileNotFoundError(f"missing required documentation: {', '.join(missing)}")
    sha, dirty = git_metadata(root)
    manifest = source_manifest("local", str(root.resolve()), sha, docs)
    manifest["source"]["dirty"] = dirty
    return docs, manifest


def source_manifest(
    mode: str, requested_ref: str, resolved_sha: str, docs: dict[str, str]
) -> dict[str, Any]:
    return {
        "source": {
            "repository": "https://github.com/trinodb/trino",
            "mode": mode,
            "requested_ref": requested_ref,
            "resolved_sha": resolved_sha,
        },
        "files": {
            path: hashlib.sha256(content.encode("utf-8")).hexdigest()
            for path, content in sorted(docs.items())
        },
    }


def render_manifest(manifest: dict[str, Any]) -> str:
    return json.dumps(manifest, indent=2, sort_keys=True) + "\n"


def check_outputs(outputs: dict[Path, str]) -> bool:
    return all(
        path.is_file() and path.read_text(encoding="utf-8") == content
        for path, content in outputs.items()
    )


def write_outputs(outputs: dict[Path, str]) -> None:
    temporary: list[tuple[Path, Path]] = []
    try:
        for path, content in outputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(
                "w", encoding="utf-8", dir=path.parent, delete=False
            ) as handle:
                handle.write(content)
                temporary.append((Path(handle.name), path))
        for source, destination in temporary:
            os.replace(source, destination)
    finally:
        for source, _ in temporary:
            source.unlink(missing_ok=True)
