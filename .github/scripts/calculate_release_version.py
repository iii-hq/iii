#!/usr/bin/env python3
"""Compute the next release version for the create-tag workflow.

Pure functions live at module top so they can be unit-tested. The CLI at
the bottom reads inputs from environment variables / git and emits the
GitHub Actions outputs the workflow needs.
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass

PRERELEASE_LABELS = ("alpha", "beta", "rc", "next")
BUMP_RANK = {"patch": 1, "minor": 2, "major": 3}

PRERELEASE_RE = re.compile(r"^(\d+\.\d+\.\d+)-([a-z]+)\.(\d+)$")
BASE_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")


@dataclass(frozen=True)
class Version:
    major: int
    minor: int
    patch: int
    prerelease_label: str | None = None
    prerelease_num: int | None = None

    @property
    def base(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}"

    def __str__(self) -> str:
        if self.prerelease_label is not None:
            return f"{self.base}-{self.prerelease_label}.{self.prerelease_num}"
        return self.base


def parse_version(s: str) -> Version:
    m = PRERELEASE_RE.match(s)
    if m:
        base, label, num = m.group(1), m.group(2), int(m.group(3))
        bm = BASE_RE.match(base)
        assert bm is not None
        return Version(int(bm.group(1)), int(bm.group(2)), int(bm.group(3)), label, num)
    bm = BASE_RE.match(s)
    if bm:
        return Version(int(bm.group(1)), int(bm.group(2)), int(bm.group(3)))
    raise ValueError(f"unrecognized version string: {s!r}")


def bump_base(base: str, bump_type: str) -> str:
    m = BASE_RE.match(base)
    if not m:
        raise ValueError(f"not a base version: {base!r}")
    major, minor, patch = int(m.group(1)), int(m.group(2)), int(m.group(3))
    if bump_type == "major":
        return f"{major + 1}.0.0"
    if bump_type == "minor":
        return f"{major}.{minor + 1}.0"
    if bump_type == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ValueError(f"unknown bump_type: {bump_type!r}")


def detect_current_level(current_base: str, latest_stable: str | None) -> str | None:
    """Classify how `current_base` relates to the last stable release.

    Returns "major" / "minor" / "patch" depending on which component grew,
    or None when we can't tell (no stable yet, or current is older).
    """
    if latest_stable is None:
        return None
    cur = parse_version(current_base)
    stable = parse_version(latest_stable)
    if cur.major > stable.major:
        return "major"
    if cur.major == stable.major and cur.minor > stable.minor:
        return "minor"
    if cur.major == stable.major and cur.minor == stable.minor and cur.patch > stable.patch:
        return "patch"
    return None


def next_prerelease_counter(
    new_base: str,
    prerelease: str,
    existing_tags: list[str],
    tag_prefix: str,
) -> int:
    """Find the next prerelease counter by scanning existing tags."""
    pattern = re.compile(
        rf"^{re.escape(tag_prefix)}/v{re.escape(new_base)}-{re.escape(prerelease)}\.(\d+)$"
    )
    nums = [int(m.group(1)) for t in existing_tags if (m := pattern.match(t))]
    return max(nums) + 1 if nums else 1


def next_dry_run_counter(base: str, existing_tags: list[str], tag_prefix: str) -> int:
    pattern = re.compile(rf"^{re.escape(tag_prefix)}/v{re.escape(base)}-dry-run\.(\d+)$")
    nums = [int(m.group(1)) for t in existing_tags if (m := pattern.match(t))]
    return max(nums) + 1 if nums else 1


def calculate_version(
    current: str,
    bump_type: str,
    prerelease: str,
    latest_stable: str | None,
    existing_tags: list[str],
    tag_prefix: str,
) -> str:
    """Decide the next version, accounting for the current prerelease train.

    Rules:
      - Promoting a prerelease to stable (prerelease == "none" and current
        has a prerelease label) keeps the base as-is.
      - `patch` always increments the patch component of the current
        base, starting a fresh prerelease counter. Patch never iterates
        an existing train.
      - `minor` / `major` in the same prerelease channel are
        level-aware: if the current base already represents a bump at
        the same or higher level (relative to the latest stable), keep
        the base and iterate the counter; if the request escalates
        beyond the current level, restart from the latest stable.
      - Otherwise apply the requested bump to the current base.
    """
    cur = parse_version(current)

    if bump_type == "none":
        # Alpha-from-stable mode: keep the base anchored on the latest
        # stable release (fall back to the current base if none exists)
        # and only iterate the prerelease counter. Never bumps the base,
        # so the official version is never advanced.
        if prerelease not in PRERELEASE_LABELS:
            raise ValueError(
                f"bump_type 'none' requires a prerelease label, got {prerelease!r}"
            )
        new_base = latest_stable if latest_stable else cur.base
        counter = next_prerelease_counter(new_base, prerelease, existing_tags, tag_prefix)
        return f"{new_base}-{prerelease}.{counter}"

    if bump_type not in BUMP_RANK:
        raise ValueError(f"unknown bump_type: {bump_type!r}")
    if prerelease != "none" and prerelease not in PRERELEASE_LABELS:
        raise ValueError(f"unknown prerelease: {prerelease!r}")

    if prerelease == "none" and cur.prerelease_label is not None:
        # Promote prerelease to stable.
        return cur.base

    same_channel = (
        prerelease != "none"
        and cur.prerelease_label == prerelease
    )
    if same_channel and bump_type != "patch":
        current_level = detect_current_level(cur.base, latest_stable)
        if current_level is not None and BUMP_RANK[bump_type] <= BUMP_RANK[current_level]:
            new_base = cur.base
        else:
            # No stable to anchor against, or the requested bump
            # escalates past the current train: restart from the
            # latest stable when available, else bump the current base.
            anchor = latest_stable if (latest_stable and current_level is not None) else cur.base
            new_base = bump_base(anchor, bump_type)
    else:
        new_base = bump_base(cur.base, bump_type)

    if prerelease == "none":
        return new_base
    counter = next_prerelease_counter(new_base, prerelease, existing_tags, tag_prefix)
    return f"{new_base}-{prerelease}.{counter}"


def to_pep440(version: str) -> str:
    m = PRERELEASE_RE.match(version)
    if not m:
        return version
    base, label, num = m.group(1), m.group(2), m.group(3)
    mapping = {"rc": f"rc{num}", "alpha": f"a{num}", "beta": f"b{num}", "next": f".dev{num}"}
    suffix = mapping.get(label)
    if suffix is None:
        return version
    return f"{base}{suffix}"


def latest_stable_from_tags(tags: list[str], tag_prefix: str) -> str | None:
    """Pick the highest stable (non-prerelease, non-dry-run) tag."""
    pattern = re.compile(rf"^{re.escape(tag_prefix)}/v(\d+\.\d+\.\d+)$")
    stables = []
    for t in tags:
        if (m := pattern.match(t)):
            stables.append(parse_version(m.group(1)))
    if not stables:
        return None
    stables.sort(key=lambda v: (v.major, v.minor, v.patch))
    return stables[-1].base


# ---------- CLI ----------


def _read_cargo_version(path: str) -> str:
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.startswith("version = "):
                return line.split('"')[1]
    raise RuntimeError(f"no version in {path}")


def _git_tags() -> list[str]:
    out = subprocess.run(
        ["git", "tag", "-l"], capture_output=True, text=True, check=True
    )
    return [t for t in out.stdout.splitlines() if t]


def _emit(name: str, value: str) -> None:
    gho = os.environ.get("GITHUB_OUTPUT")
    if gho:
        with open(gho, "a", encoding="utf-8") as f:
            f.write(f"{name}={value}\n")
    print(f"{name}={value}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--bump", required=True, choices=list(BUMP_RANK) + ["none"])
    parser.add_argument("--prerelease", required=True)
    parser.add_argument(
        "--counter-tag-prefix",
        default=None,
        help=(
            "Tag prefix used when scanning for the prerelease counter "
            "(default: --target). Lets alpha releases accumulate under a "
            "separate namespace, e.g. iii-alpha, without colliding with the "
            "official iii/v* tags."
        ),
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--current-version-file", required=True)
    args = parser.parse_args(argv)

    current = _read_cargo_version(args.current_version_file)
    tags = _git_tags()
    stable_prefix = args.target
    counter_prefix = args.counter_tag_prefix or args.target
    latest_stable = latest_stable_from_tags(tags, stable_prefix)

    new_ver = calculate_version(
        current=current,
        bump_type=args.bump,
        prerelease=args.prerelease,
        latest_stable=latest_stable,
        existing_tags=tags,
        tag_prefix=counter_prefix,
    )

    if args.dry_run:
        base_ver = new_ver.split("-", 1)[0]
        counter = next_dry_run_counter(base_ver, tags, counter_prefix)
        new_ver = f"{base_ver}-dry-run.{counter}"

    py_ver = to_pep440(new_ver)
    is_prerelease = args.prerelease != "none"
    npm_tag = args.prerelease if is_prerelease else "latest"

    _emit("version", new_ver)
    _emit("python_version", py_ver)
    _emit("tag", f"{counter_prefix}/v{new_ver}")
    _emit("current", current)
    _emit("is_prerelease", "true" if is_prerelease else "false")
    _emit("npm_tag", npm_tag)
    print(f"::notice::{counter_prefix}: {current} -> {new_ver} (python: {py_ver})", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
