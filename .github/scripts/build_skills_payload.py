#!/usr/bin/env python3
"""Build the POST /w/<slug>/skills payload from a worker directory.

Walks ``<worker>/skill.md`` and ``<worker>/skills/**/*.md`` and produces the JSON
body expected by the workers-registry endpoint.  Skill paths map to keys as:

    <worker>/skill.md             -> "index.md"
    <worker>/skills/index.md      -> "index.md"   (override; warn if both exist)
    <worker>/skills/<rel>.md      -> "skills/<rel>.md"

If no non-empty markdown is found the script writes ``skip=true`` to
``$GITHUB_OUTPUT`` (so the calling workflow can gate the POST step off) and
exits cleanly; the API rejects payloads that omit both ``skills`` and
``prompts``, and ``skills: {}`` would be a destructive "clear all" call which is
wrong on a fresh publish.
"""
import argparse
import json
import os
import pathlib
import re
import sys


KEY_RE = re.compile(r"^[a-z0-9][a-z0-9._/\-]*\.md$", re.IGNORECASE)


def collect_skills(worker_root: pathlib.Path) -> dict[str, str]:
    """Return a ``{payload-key: markdown-body}`` map for one worker directory.

    The top-of-tree resolution order is ``skills/index.md`` then ``skill.md``;
    if both exist, a GitHub Actions warning is emitted and the nested one wins
    (this matches ``iii-directory``'s on-disk convention).  Empty bodies are
    skipped silently so blank placeholder files don't end up in the registry.
    """
    skills: dict[str, str] = {}

    leaves_dir = worker_root / "skills"
    skills_index = leaves_dir / "index.md"
    intro = worker_root / "skill.md"

    if skills_index.is_file():
        body = skills_index.read_text(encoding="utf-8")
        if body.strip():
            skills["index.md"] = body
        if intro.is_file():
            print(
                f"::warning::{worker_root.name}: both skill.md and "
                "skills/index.md present; using skills/index.md as the "
                "top-of-tree."
            )
    elif intro.is_file():
        body = intro.read_text(encoding="utf-8")
        if body.strip():
            skills["index.md"] = body

    if leaves_dir.is_dir():
        for path in sorted(leaves_dir.rglob("*.md")):
            if path == skills_index:
                continue
            rel = path.relative_to(worker_root).as_posix()
            if not KEY_RE.match(rel):
                raise ValueError(
                    f"skill path rejected by server regex: {rel} "
                    "(must match /^[a-z0-9][a-z0-9._/\\-]*\\.md$/i)"
                )
            body = path.read_text(encoding="utf-8")
            if not body.strip():
                continue
            skills[rel] = body

    return skills


def _signal_skip(worker: str) -> None:
    gha_out = os.environ.get("GITHUB_OUTPUT")
    if gha_out:
        with open(gha_out, "a", encoding="utf-8") as f:
            f.write("skip=true\n")
    print(f"::notice::no skills found for {worker}; skipping POST /w/.../skills")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--worker-dir",
        help="Path to worker source directory relative to repo root "
        "(e.g. engine/src/workers/state).",
    )
    parser.add_argument(
        "--worker",
        help="(deprecated) Same as --worker-dir.",
    )
    parser.add_argument(
        "--version",
        required=True,
        help="Worker version tag or semver to attach this snapshot to (e.g. latest, next, 1.2.3).",
    )
    parser.add_argument("--out", default="skills-payload.json")
    parser.add_argument(
        "--repo-root",
        default=".",
        help="Repo root containing the worker folder (default: cwd).",
    )
    args = parser.parse_args()

    worker_dir = args.worker_dir or args.worker
    if not worker_dir:
        parser.error("one of --worker-dir or --worker is required")

    worker_root = pathlib.Path(args.repo_root) / worker_dir
    if not worker_root.is_dir():
        print(f"::error::worker directory not found: {worker_root}", file=sys.stderr)
        return 1

    try:
        skills = collect_skills(worker_root)
    except ValueError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        return 1

    out_path = pathlib.Path(args.out)
    if not skills:
        out_path.write_text("{}\n", encoding="utf-8")
        _signal_skip(worker_dir)
        return 0

    payload = {"version": args.version, "skills": skills}
    out_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"::notice::collected {len(skills)} skill file(s) for {worker_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
