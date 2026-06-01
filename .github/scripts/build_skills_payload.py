#!/usr/bin/env python3
"""Build the POST /w/<slug>/skills payload from a worker directory.

Walks the worker's top-of-tree skill doc plus ``<worker>/skills/**/*.md`` and
produces the JSON body expected by the workers-registry endpoint.  Skill paths
map to payload keys as:

    <worker>/skills/SKILL.md      -> "index.md"   (preferred top-of-tree)
    <worker>/skills/index.md      -> "index.md"   (legacy)
    <worker>/skill.md             -> "index.md"   (legacy)
    <worker>/skills/<rel>.md      -> "skills/<rel>.md"

When more than one top-of-tree candidate is present the highest-precedence one
wins (``skills/SKILL.md`` > ``skills/index.md`` > ``skill.md``) and a GitHub
Actions warning is emitted.

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

    The top-of-tree resolution order is ``skills/SKILL.md`` then
    ``skills/index.md`` then ``skill.md``; whichever wins is published under the
    ``index.md`` payload key.  If more than one is present a GitHub Actions
    warning is emitted and the highest-precedence file wins.  Empty bodies are
    skipped silently so blank placeholder files don't end up in the registry.
    """
    skills: dict[str, str] = {}

    leaves_dir = worker_root / "skills"
    skill_md = leaves_dir / "SKILL.md"
    skills_index = leaves_dir / "index.md"
    intro = worker_root / "skill.md"

    # Preferred new convention is skills/SKILL.md; the skills/index.md and
    # skill.md forms are legacy. Whichever wins maps to the "index.md" key.
    top_candidates = [c for c in (skill_md, skills_index, intro) if c.is_file()]
    if top_candidates:
        top = top_candidates[0]
        body = top.read_text(encoding="utf-8")
        if body.strip():
            skills["index.md"] = body
        if len(top_candidates) > 1:
            present = ", ".join(
                c.relative_to(worker_root).as_posix() for c in top_candidates
            )
            print(
                f"::warning::{worker_root.name}: multiple top-of-tree skill "
                f"files present ({present}); using "
                f"{top.relative_to(worker_root).as_posix()} as the top-of-tree."
            )

    if leaves_dir.is_dir():
        for path in sorted(leaves_dir.rglob("*.md")):
            if path in (skill_md, skills_index):
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
