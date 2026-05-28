#!/usr/bin/env python3
"""Validate release-iii publish-builtin-workers matrix against worker manifests.

Checks that every matrix entry points at an engine worker manifest and that
registry slugs (manifest ``name``) stay aligned with what skill discovery emits.
Matrix ``worker`` values must remain the engine config / runtime names used by
``_publish-engine-workers.yml`` when reloading config.yaml.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import sys

import yaml

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from discover_engine_worker_skills import discover  # noqa: E402

_MATRIX_BLOCK_RE = re.compile(
    r"publish-builtin-workers:.*?matrix:\s*\n\s*include:(.*?)(?:\n\s{4}\w|\n\s{2}\w|\Z)",
    re.DOTALL,
)
_MATRIX_ENTRY_RE = re.compile(
    r"- worker: (?P<worker>.+)\n\s+worker_dir: (?P<worker_dir>.+)"
)


def _load_matrix(repo_root: pathlib.Path) -> list[dict[str, str]]:
    workflow = (repo_root / ".github/workflows/release-iii.yml").read_text(encoding="utf-8")
    block = _MATRIX_BLOCK_RE.search(workflow)
    if not block:
        raise ValueError("could not find publish-builtin-workers matrix in release-iii.yml")
    return [
        {"worker": m.group("worker").strip(), "worker_dir": m.group("worker_dir").strip()}
        for m in _MATRIX_ENTRY_RE.finditer(block.group(1))
    ]


def validate(repo_root: pathlib.Path) -> list[str]:
    errors: list[str] = []
    matrix = _load_matrix(repo_root)
    discovered = {entry["worker_dir"]: entry["slug"] for entry in discover(repo_root)}

    for entry in matrix:
        worker = entry["worker"]
        worker_dir = entry["worker_dir"]
        manifest_path = repo_root / worker_dir / "iii.worker.yaml"
        if not manifest_path.is_file():
            errors.append(f"{worker_dir}: missing iii.worker.yaml for matrix worker {worker!r}")
            continue

        meta = yaml.safe_load(manifest_path.read_text(encoding="utf-8")) or {}
        if meta.get("type") != "engine":
            errors.append(
                f"{manifest_path}: type must be 'engine' (got {meta.get('type')!r})"
            )

        registry_name = meta.get("name")
        if not isinstance(registry_name, str) or not registry_name.strip():
            errors.append(f"{manifest_path}: missing or empty name field")
            continue

        registry_name = registry_name.strip()
        if worker_dir in discovered and discovered[worker_dir] != registry_name:
            errors.append(
                f"{worker_dir}: discover slug {discovered[worker_dir]!r} != "
                f"manifest name {registry_name!r}"
            )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=".", help="Repository root (default: cwd).")
    args = parser.parse_args()

    repo_root = pathlib.Path(args.repo_root).resolve()
    errors = validate(repo_root)
    if errors:
        for err in errors:
            print(f"::error::{err}", file=sys.stderr)
        return 1

    print("::notice::release worker matrix validation passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
