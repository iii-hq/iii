#!/usr/bin/env python3
"""Validate release publish-builtin-workers matrices against worker manifests.

Checks that the stable and alpha matrices stay aligned, every entry points at
an engine worker manifest, and every publishable engine worker is present.
Registry slugs (manifest ``name``) must stay aligned with what skill discovery
emits. Matrix ``worker`` values remain the engine config / runtime names used
by ``_publish-engine-workers.yml`` when reloading config.yaml.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

import yaml

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from discover_engine_worker_skills import discover

_MATRIX_BLOCK_RE = re.compile(
    r"publish-builtin-workers:.*?matrix:\s*\n\s*include:(.*?)(?:\n\s{4}\w|\n\s{2}\w|\Z)",
    re.DOTALL,
)
_MATRIX_ENTRY_RE = re.compile(
    r"- worker: (?P<worker>.+)\n\s+worker_dir: (?P<worker_dir>.+)"
)
_INLINE_MATRIX_ENTRY_RE = re.compile(
    r"- \{ worker: (?P<worker>[^,]+), worker_dir: (?P<worker_dir>[^}]+) \}"
)

_WORKFLOW_NAMES = ("release-iii.yml", "alpha-release.yml")


def _load_matrix(repo_root: pathlib.Path, workflow_name: str) -> list[dict[str, str]]:
    workflow = (repo_root / ".github/workflows" / workflow_name).read_text(
        encoding="utf-8"
    )
    block = _MATRIX_BLOCK_RE.search(workflow)
    if not block:
        raise ValueError(
            f"could not find publish-builtin-workers matrix in {workflow_name}"
        )

    entries = [
        {
            "worker": m.group("worker").strip(),
            "worker_dir": m.group("worker_dir").strip(),
        }
        for m in _MATRIX_ENTRY_RE.finditer(block.group(1))
    ]
    entries.extend(
        {
            "worker": m.group("worker").strip(),
            "worker_dir": m.group("worker_dir").strip(),
        }
        for m in _INLINE_MATRIX_ENTRY_RE.finditer(block.group(1))
    )
    if not entries:
        raise ValueError(f"publish-builtin-workers matrix is empty in {workflow_name}")
    return entries


def validate(repo_root: pathlib.Path) -> list[str]:
    errors: list[str] = []
    discovered = {entry["worker_dir"]: entry["slug"] for entry in discover(repo_root)}
    matrices: dict[str, list[dict[str, str]]] = {}

    for workflow_name in _WORKFLOW_NAMES:
        try:
            matrix = _load_matrix(repo_root, workflow_name)
        except ValueError as exc:
            errors.append(str(exc))
            continue

        matrices[workflow_name] = matrix
        matrix_dirs = [entry["worker_dir"] for entry in matrix]

        for worker_dir in sorted(set(discovered) - set(matrix_dirs)):
            errors.append(
                f"{workflow_name}: missing publishable engine worker {worker_dir!r}"
            )

        for worker_dir in sorted(
            worker_dir
            for worker_dir in set(matrix_dirs)
            if matrix_dirs.count(worker_dir) > 1
        ):
            errors.append(f"{workflow_name}: duplicate worker directory {worker_dir!r}")

        for entry in matrix:
            worker = entry["worker"]
            worker_dir = entry["worker_dir"]
            manifest_path = repo_root / worker_dir / "iii.worker.yaml"
            if not manifest_path.is_file():
                errors.append(
                    f"{workflow_name}: {worker_dir}: missing iii.worker.yaml "
                    f"for matrix worker {worker!r}"
                )
                continue

            meta = yaml.safe_load(manifest_path.read_text(encoding="utf-8")) or {}
            if meta.get("type") != "engine":
                errors.append(
                    f"{workflow_name}: {manifest_path}: type must be 'engine' "
                    f"(got {meta.get('type')!r})"
                )

            registry_name = meta.get("name")
            if not isinstance(registry_name, str) or not registry_name.strip():
                errors.append(
                    f"{workflow_name}: {manifest_path}: missing or empty name field"
                )
                continue

            registry_name = registry_name.strip()
            if worker_dir in discovered and discovered[worker_dir] != registry_name:
                errors.append(
                    f"{workflow_name}: {worker_dir}: discover slug "
                    f"{discovered[worker_dir]!r} != manifest name {registry_name!r}"
                )

    if len(matrices) == len(_WORKFLOW_NAMES):
        release_entries = {
            (entry["worker"], entry["worker_dir"])
            for entry in matrices["release-iii.yml"]
        }
        alpha_entries = {
            (entry["worker"], entry["worker_dir"])
            for entry in matrices["alpha-release.yml"]
        }
        if release_entries != alpha_entries:
            errors.append(
                "release-iii.yml and alpha-release.yml publish different engine workers"
            )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repo-root", default=".", help="Repository root (default: cwd)."
    )
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
