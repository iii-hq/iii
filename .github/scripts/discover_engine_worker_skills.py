#!/usr/bin/env python3
"""Discover engine workers with publishable skill bundles.

Scans ``engine/src/workers/*/skills/`` and emits a GitHub Actions matrix JSON
with ``worker_dir`` (path relative to repo root) and ``slug`` (registry name
from ``iii.worker.yaml``). Workers whose skills tree has no non-empty markdown
are omitted.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from build_skills_payload import collect_skills  # noqa: E402


def _read_slug(manifest_path: pathlib.Path) -> str:
    try:
        import yaml
    except ImportError as exc:
        raise RuntimeError("pyyaml is required: pip install pyyaml") from exc

    data = yaml.safe_load(manifest_path.read_text(encoding="utf-8")) or {}
    name = data.get("name")
    if not isinstance(name, str) or not name.strip():
        raise ValueError(f"{manifest_path}: missing or empty `name` field")
    return name.strip()


def discover(repo_root: pathlib.Path) -> list[dict[str, str]]:
    workers_root = repo_root / "engine" / "src" / "workers"
    if not workers_root.is_dir():
        return []

    entries: list[dict[str, str]] = []
    for skills_dir in sorted(workers_root.glob("*/skills")):
        if not skills_dir.is_dir():
            continue

        worker_dir = skills_dir.parent
        manifest = worker_dir / "iii.worker.yaml"
        if not manifest.is_file():
            print(
                f"::warning::{worker_dir.relative_to(repo_root)}: "
                "skills/ present but iii.worker.yaml missing; skipping"
            )
            continue

        try:
            slug = _read_slug(manifest)
            skills = collect_skills(worker_dir)
        except ValueError as exc:
            print(f"::error::{exc}", file=sys.stderr)
            raise

        if not skills:
            print(
                f"::notice::{worker_dir.relative_to(repo_root)}: "
                "no non-empty skill markdown; skipping"
            )
            continue

        entries.append(
            {
                "worker_dir": worker_dir.relative_to(repo_root).as_posix(),
                "slug": slug,
            }
        )

    return entries


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repo-root",
        default=".",
        help="Repository root (default: cwd).",
    )
    parser.add_argument(
        "--out",
        default="-",
        help="Write matrix JSON here, or '-' for stdout (default).",
    )
    args = parser.parse_args()

    repo_root = pathlib.Path(args.repo_root).resolve()
    matrix = {"include": discover(repo_root)}
    text = json.dumps(matrix)

    if args.out == "-":
        print(text)
    else:
        pathlib.Path(args.out).write_text(text + "\n", encoding="utf-8")

    gha_out = os.environ.get("GITHUB_OUTPUT")
    if gha_out:
        with open(gha_out, "a", encoding="utf-8") as f:
            f.write(f"matrix={text}\n")
            f.write(f"has_workers={'true' if matrix['include'] else 'false'}\n")

    print(
        f"::notice::discovered {len(matrix['include'])} worker(s) with publishable skills"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
