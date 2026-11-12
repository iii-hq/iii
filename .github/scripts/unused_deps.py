#!/usr/bin/env python3
"""Report unused dependencies as GitHub warning annotations, never as a failure.

    unused_deps.py TOOL [--directory DIR] [-- EXTRA_TOOL_ARGS...]

TOOL is one of:

  cargo-shear  Rust. Runs `cargo shear --locked --format=github`; cargo-shear
               must be on PATH.
  knip         Node. Runs knip (pinned below, through npx) for unused
               dependencies and devDependencies. The project's dependencies
               must be installed; knip.json tunes entries and ignores.
  deptry       Python. Runs `deptry` for unused (DEP002) and misplaced dev
               (DEP004) dependencies, treating the `dev` extra as development
               dependencies. Run this script with the project's environment
               (for example `uv run --with deptry python3 unused_deps.py ...`).
  go           Go. Runs `go mod tidy -diff`; every module the diff drops from
               go.mod is a finding.

The check runs in DIR (default: the current directory) and every annotation
path is prefixed with DIR, so it lands on the right file. Error-level findings
are downgraded to warnings, a check that cannot run ends as one warning that
points at the log, and the script always exits 0: unused dependencies never
fail CI.

This file is shared verbatim by iii-hq/iii and iii-hq/workers.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import PurePosixPath
import re
import shlex
import subprocess
import sys

KNIP_VERSION = "6.45.1"


@dataclass(frozen=True)
class Tool:
    command: tuple[str, ...]
    hint: str


TOOLS = {
    "cargo-shear": Tool(
        ("cargo", "shear", "--locked", "--format=github"),
        "Run 'cargo shear --fix', or list intentional ones under ignored in "
        "[package.metadata.cargo-shear].",
    ),
    "knip": Tool(
        (
            "npx", "--yes", f"knip@{KNIP_VERSION}",
            "--include", "dependencies",
            "--no-config-hints", "--no-progress",
            "--reporter", "github-actions",
        ),
        "Remove them from package.json, or list intentional ones under "
        "ignoreDependencies in knip.jsonc.",
    ),
    "deptry": Tool(
        (
            "deptry", ".",
            "--pep621-dev-dependency-groups", "dev",
            # Only unused (DEP002) and misplaced dev (DEP004) dependencies:
            # missing (DEP001) and transitive (DEP003) imports are a separate
            # concern.
            "--ignore", "DEP001,DEP003",
            "--github-output",
        ),
        "Remove them from pyproject.toml, or list intentional ones under "
        "[tool.deptry.per_rule_ignores].",
    ),
    "go": Tool(("go", "mod", "tidy", "-diff"), "Run 'go mod tidy'."),
}

# A GitHub workflow command: ::level key=value,key=value::message
ANNOTATION = re.compile(r"^::(error|warning|notice)(?: (.*?))?::(.*)$")
HUNK = re.compile(r"^@@ -(\d+)(?:,\d+)? \+\d+(?:,\d+)? @@")
# A go.mod requirement, alone (`require m v1`) or inside a require block.
REQUIREMENT = re.compile(r"^(?:require\s+)?([^\s()]+)\s+(v\S+)")


def rewrite(line: str, directory: str) -> str | None:
    """Return `line` as a warning annotation on a repository path, or None."""
    match = ANNOTATION.match(line)
    if match is None:
        return None
    level, properties, message = match.groups()
    if level == "error":
        level = "warning"
    rewritten = []
    for prop in properties.split(",") if properties else []:
        key, _, value = prop.partition("=")
        if key == "file":
            value = str(PurePosixPath(directory) / value)
        rewritten.append(f"{key}={value}")
    if rewritten:
        return f"::{level} {','.join(rewritten)}::{message}"
    return f"::{level}::{message}"


def go_mod_tidy_annotations(diff: str) -> list[str]:
    """Turn `go mod tidy -diff` output into one annotation per unused module.

    A module the diff removes from go.mod without adding it back (a version
    or `// indirect` change removes and re-adds it) is unused. A diff with no
    such module still differs from `go mod tidy`, which is reported once.
    """
    current = None
    old_line = 0
    removed: dict[str, tuple[int, str]] = {}
    added: set[str] = set()
    changed: list[str] = []
    for line in diff.splitlines():
        if line.startswith("--- "):
            current = PurePosixPath(line[4:].strip()).name
            continue
        if line.startswith(("+++ ", "diff ")):
            continue
        hunk = HUNK.match(line)
        if hunk:
            old_line = int(hunk.group(1))
            continue
        # An empty line is a context line whose trailing space was stripped.
        if current is None or line[:1] not in {"-", "+", " ", ""}:
            continue
        requirement = REQUIREMENT.match(line[1:].strip()) if current == "go.mod" else None
        if line.startswith("-"):
            if requirement:
                removed.setdefault(requirement.group(1), (old_line, requirement.group(2)))
            if current not in changed:
                changed.append(current)
            old_line += 1
        elif line.startswith("+"):
            if requirement:
                added.add(requirement.group(1))
            if current not in changed:
                changed.append(current)
        else:
            old_line += 1
    annotations = [
        f"::error file=go.mod,line={line},title=go mod tidy::unused module {module} {version}"
        for module, (line, version) in removed.items()
        if module not in added
    ]
    if not annotations and changed:
        annotations.append(
            f"::error file={changed[0]},title=go mod tidy::"
            f"{' and '.join(changed)} differ from 'go mod tidy'"
        )
    return annotations


def run(name: str, directory: str, extra: list[str]) -> None:
    tool = TOOLS[name]
    directory = str(PurePosixPath(directory))
    where = "repository root" if directory == "." else directory
    command = [*tool.command, *extra]
    print(f"$ {shlex.join(command)}  # in {where}", flush=True)
    try:
        result = subprocess.run(
            command, cwd=directory, capture_output=True, text=True, check=False
        )
        output, status = result.stdout + result.stderr, result.returncode
    except OSError as error:
        output, status = f"{error}\n", 127

    lines = go_mod_tidy_annotations(output) if name == "go" else output.splitlines()
    findings = [annotation for line in lines if (annotation := rewrite(line, directory))]
    title = f"title={where}: unused dependencies ({name})"
    if findings:
        print("\n".join(findings))
        count = "1 finding" if len(findings) == 1 else f"{len(findings)} findings"
        print(f"::warning {title}::{name} reported {count} in {where}. {tool.hint}")
    elif status != 0:
        # Never echo the tool's own workflow commands: they would surface as
        # errors. Anything else is the log the warning points at.
        print("\n".join(line for line in output.splitlines() if not line.startswith("::")))
        print(f"::warning {title}::{name} could not run in {where} (exit {status}); see the log above.")
    else:
        print(f"{where}: no unused dependencies ({name}).")


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    extra: list[str] = []
    if "--" in args:
        split = args.index("--")
        args, extra = args[:split], args[split + 1:]
    directory = "."
    if "--directory" in args:
        index = args.index("--directory")
        directory = args[index + 1] if index + 1 < len(args) else ""
        del args[index:index + 2]
    if len(args) != 1 or args[0] not in TOOLS or not directory:
        print(f"usage: unused_deps.py {{{','.join(TOOLS)}}} [--directory DIR] [-- ARGS...]")
        print("::warning title=unused dependencies::unused_deps.py was called with invalid arguments.")
        return 0
    try:
        run(args[0], directory, extra)
    except Exception as error:  # noqa: BLE001 - a warning-only check never fails CI
        print(f"::warning title=unused dependencies::unused_deps.py failed: {error!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
