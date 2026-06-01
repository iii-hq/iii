#!/usr/bin/env python3
"""Rotate Mintlify docs versions during the create-tag release flow.

On a stable minor/major release: archive the old Latest (root) into a
MAJOR-MINOR-0/ folder, promote the fixed docs/next/ folder into the root as
the new Latest (labeled with the tag version), and relabel the Next block to
the following minor.

Two subcommands:
* ``validate`` — gate a stable release on the Next docs being ready (a Next
  block exists and docs/next/ is non-empty). Runs even on dry runs.
* ``rotate`` — update docs from docs/next/. Dispatches on the version: a
  minor/major does a full rotation (archive + promote + bump Next); a patch
  refreshes the current Latest in place (no archive, no Next bump).

Pure functions live at module top so they can be unit-tested. The CLI at the
bottom is invoked from .github/workflows/create-tag.yml.
"""
from __future__ import annotations

import argparse
import copy
import json
import re
import shutil
import sys
from pathlib import Path

NEXT_TAG = "Next"
LATEST_TAG = "Latest"
NEXT_DIR = "next"  # fixed folder name holding the Next docs

_VERSION_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)")


def parse_version(version: str) -> tuple[int, int, int]:
    """Return (major, minor, patch); any prerelease/dry-run suffix is ignored."""
    m = _VERSION_RE.match(version)
    if not m:
        raise ValueError(f"unrecognized version string: {version!r}")
    return int(m.group(1)), int(m.group(2)), int(m.group(3))


def minor_label(major: int, minor: int) -> str:
    return f"{major}.{minor}.x"


def dir_prefix(major: int, minor: int) -> str:
    return f"{major}-{minor}-0"


def next_minor(version: str) -> tuple[int, int]:
    """Return (major, minor+1) for the version — the new Next after a release."""
    major, minor, _ = parse_version(version)
    return major, minor + 1


def _map_page_strings(node, fn, in_pages=False):
    if isinstance(node, str):
        return fn(node) if in_pages else node
    if isinstance(node, list):
        return [_map_page_strings(item, fn, in_pages) for item in node]
    if isinstance(node, dict):
        return {k: _map_page_strings(v, fn, k == "pages") for k, v in node.items()}
    return node


def _is_shared_page(path: str) -> bool:
    return path.split("/", 1)[0] in SHARED_ROOTS


def add_prefix(tabs: list, prefix: str) -> list:
    """Deep-copy ``tabs`` and prefix every page string with ``prefix/``, except
    shared-root pages (e.g. changelog), which always stay at the root."""
    return _map_page_strings(
        copy.deepcopy(tabs),
        lambda s: s if _is_shared_page(s) else f"{prefix}/{s}",
    )


def strip_prefix(tabs: list, prefix: str) -> list:
    """Deep-copy ``tabs`` and remove a leading ``prefix/`` from page strings."""
    head = f"{prefix}/"
    return _map_page_strings(
        copy.deepcopy(tabs),
        lambda s: s[len(head):] if s.startswith(head) else s,
    )


def find_index_by_tag(versions: list[dict], tag: str) -> int:
    for i, block in enumerate(versions):
        if block.get("tag") == tag:
            return i
    raise ValueError(f"no version block tagged {tag!r} in docs.json")


_MINOR_RE = re.compile(r"^(\d+)\.(\d+)")


def _version_sort_key(block: dict) -> tuple[int, int]:
    m = _MINOR_RE.match(block.get("version", ""))
    return (int(m.group(1)), int(m.group(2))) if m else (-1, -1)


def sort_versions(versions: list[dict]) -> list[dict]:
    """Order: Next first, then Latest, then archived newest-first (Mintlify
    renders the dropdown in array order)."""
    nxt = [b for b in versions if b.get("tag") == NEXT_TAG]
    latest = [b for b in versions if b.get("tag") == LATEST_TAG]
    rest = [b for b in versions if b.get("tag") not in (NEXT_TAG, LATEST_TAG)]
    rest.sort(key=_version_sort_key, reverse=True)
    return nxt + latest + rest


_VERSION_DIR_RE = re.compile(r"^\d+-\d+-\d+$")

# Content shared across all versions: kept at the docs root, never copied into a
# version folder and never version-prefixed in docs.json (every version's tab
# points at the single root copy).
SHARED_ROOTS = {"changelog"}

_EXCLUDED_NAMES = {
    "docs.json", "package.json", "package-lock.json", "node_modules",
    "custom.css", "navbar-counters.js", ".gitignore", ".mintignore",
    ".prettierrc", "README.md", "LICENSE", NEXT_DIR,
}


def is_excluded(name: str) -> bool:
    """True if a top-level docs entry is not movable content (infra, sidecars,
    version folders, the fixed next/ folder, or shared root content)."""
    return (
        name in _EXCLUDED_NAMES
        or name in SHARED_ROOTS
        or name.endswith(".skill.md")
        or bool(_VERSION_DIR_RE.match(name))
    )


def _content_entries(docs_dir: Path):
    return [e for e in sorted(docs_dir.iterdir()) if not is_excluded(e.name)]


def _copy_entry(src: Path, dst: Path) -> None:
    if src.is_dir():
        shutil.copytree(src, dst)
    else:
        shutil.copy2(src, dst)


def copy_root_to_dir(docs_dir: Path, prefix: str) -> None:
    """Copy root content (excluding infra/version/next) into ``docs_dir/prefix``,
    overwriting it."""
    target = docs_dir / prefix
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    for entry in _content_entries(docs_dir):
        _copy_entry(entry, target / entry.name)


def replace_root_with_dir(docs_dir: Path, prefix: str) -> None:
    """Replace root content with the content of ``docs_dir/prefix`` (infra,
    version dirs, and the next/ folder are left in place). The source folder is
    not removed."""
    source = docs_dir / prefix
    for entry in _content_entries(docs_dir):
        if entry.is_dir():
            shutil.rmtree(entry)
        else:
            entry.unlink()
    for entry in sorted(source.iterdir()):
        _copy_entry(entry, docs_dir / entry.name)


def _load(docs_dir: Path) -> dict:
    return json.loads((docs_dir / "docs.json").read_text(encoding="utf-8"))


def _save(docs_dir: Path, doc: dict) -> None:
    (docs_dir / "docs.json").write_text(
        json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


def validate(docs_dir: Path) -> int:
    """Exit 0 if the Next docs are ready; 1 (with error) otherwise.

    Ready = a Next block exists in docs.json AND docs/next/ is non-empty. The
    release version is irrelevant — rotation always pulls whatever is in next/.
    """
    try:
        versions = _load(docs_dir)["navigation"]["versions"]
        find_index_by_tag(versions, NEXT_TAG)
    except (FileNotFoundError, ValueError) as exc:
        print(f"::error::Cannot validate Next docs: {exc}")
        return 1

    folder = docs_dir / NEXT_DIR
    if not folder.is_dir() or not any(folder.iterdir()):
        print(
            f"::error::Missing Next docs at {folder}. "
            f"Prepare docs/{NEXT_DIR}/ before tagging."
        )
        return 1

    print("pin_docs: Next docs ready")
    return 0


def rotate(docs_dir: Path, version: str) -> None:
    """Archive the old Latest, promote docs/next/ into root as the new Latest
    (labeled with the tag version), and relabel the Next block to minor+1.

    Assumes ``validate`` already passed.
    """
    major, minor, patch = parse_version(version)
    # A patch release does not rotate — it only refreshes the current Latest
    # from docs/next/ in place (no archive, no Next bump, no version changes).
    if patch > 0:
        sync_patch(docs_dir)
        return
    rel_label = minor_label(major, minor)        # tag version label, e.g. 0.17.x
    nmaj, nmin = next_minor(version)
    nxt_label = minor_label(nmaj, nmin)          # e.g. 0.18.x

    doc = _load(docs_dir)
    versions = doc["navigation"]["versions"]
    next_block = versions[find_index_by_tag(versions, NEXT_TAG)]
    latest_block = versions[find_index_by_tag(versions, LATEST_TAG)]

    # Idempotency guard: if the current Latest is already this version, the
    # rotation has already happened for it — re-running would duplicate the
    # block and re-copy folders. No-op.
    if latest_block["version"] == rel_label:
        print(f"pin_docs: already rotated to {rel_label}, skipping")
        return

    old_major, old_minor, _ = parse_version(latest_block["version"].replace("x", "0"))
    old_prefix = dir_prefix(old_major, old_minor)  # e.g. 0-16-0

    # --- filesystem (order matters) ---
    # In-content links are version-relative, so they survive the move untouched.
    copy_root_to_dir(docs_dir, old_prefix)          # archive old Latest -> folder
    replace_root_with_dir(docs_dir, NEXT_DIR)       # promote next/ -> root (next/ kept)

    # --- docs.json ---
    # old Latest -> archived (root paths -> old_prefix), drop tag/default
    latest_block["tabs"] = add_prefix(latest_block["tabs"], old_prefix)
    latest_block.pop("tag", None)
    latest_block.pop("default", None)
    # new Latest block from the Next tabs (next/ -> root), labeled with the tag
    new_latest = {
        "version": rel_label,
        "tag": LATEST_TAG,
        "default": True,
        "tabs": strip_prefix(next_block["tabs"], NEXT_DIR),
    }
    # Next block keeps its next/ paths; only its label advances to minor+1
    next_block["version"] = nxt_label

    versions.append(new_latest)
    doc["navigation"]["versions"] = sort_versions(versions)
    _save(docs_dir, doc)
    print(f"pin_docs: rotated — Latest={rel_label}, Next={nxt_label}, archived {latest_block['version']}")


def sync_patch(docs_dir: Path) -> None:
    """Patch release: refresh the Latest (root) docs from docs/next/ in place.

    Unlike ``rotate`` there is no version ceremony — the old Latest is not
    archived, the Next block is not bumped, and no version blocks in docs.json
    change. Only the root content is replaced (in-content links are relative, so
    they survive the move untouched).
    """
    replace_root_with_dir(docs_dir, NEXT_DIR)
    print("pin_docs: patched Latest from docs/next/ (no rotation)")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)

    p_validate = sub.add_parser("validate")
    p_validate.add_argument("--docs-dir", required=True, type=Path)

    p_rotate = sub.add_parser("rotate")
    p_rotate.add_argument("--docs-dir", required=True, type=Path)
    p_rotate.add_argument("--version", required=True)

    args = parser.parse_args(argv)
    if args.command == "validate":
        return validate(args.docs_dir)
    rotate(args.docs_dir, args.version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
