#!/usr/bin/env python3
"""Enable the Linkly tutorial chapters 1..N in a scaffolded project.

Uncomments the compose blocks for those chapters, and in every worker source
uncomments the newest variant of each code block at or below chapter N.
"""

import pathlib
import re
import sys

YAML_LINE = re.compile(r"^\s*(-\s|[A-Za-z][\w.#-]*:)")
CHAPTER_HEADING = re.compile(r"^\s*# (Ch\. (\d+)|Agentic path):")
BLOCK_HEADER = re.compile(
    r"^(?P<indent>\s*)(?P<comment>//|#) --- Ch\. (?P<chapter>\d+) \| (?P<key>.+?)"
    r"(?: \(replaces Ch\. \d+\))? ---\s*$"
)
YAML_FILES = ["worker-compose.yaml"]
SOURCES = [
    "link/src/index.ts",
    "analytics/src/main.py",
    "click-streamer/src/index.ts",
    "bulk-importer/src/index.ts",
    "auth/src/index.ts",
    "channel-client/import-links.js",
]


def strip_comment(line: str, marker: str) -> str:
    body = line.lstrip()
    if not body.startswith(marker):
        return line
    indent = line[: len(line) - len(body)]
    body = body[len(marker) :]
    if body.startswith(" "):
        body = body[1:]
    return indent + body


def enable_yaml(path: pathlib.Path, upto: int, agentic: bool) -> None:
    if not path.exists():
        return
    out = []
    active = False
    for line in path.read_text().splitlines():
        heading = CHAPTER_HEADING.match(line)
        if heading:
            active = agentic if heading.group(1) == "Agentic path" else int(heading.group(2)) <= upto
            out.append(line)
            continue
        body = line.lstrip()
        if active and body.startswith("# ") and YAML_LINE.match(body[2:]):
            out.append(strip_comment(line, "#"))
        else:
            out.append(line)
    path.write_text("\n".join(out) + "\n")


def enable_source(path: pathlib.Path, upto: int) -> None:
    if not path.exists():
        return
    lines = path.read_text().splitlines()
    blocks = []
    for index, line in enumerate(lines):
        header = BLOCK_HEADER.match(line)
        if header:
            blocks.append((index, int(header.group("chapter")), header.group("key"), header.group("comment")))

    # The newest variant of each key at or below `upto` wins.
    chosen = {}
    for index, chapter, key, marker in blocks:
        if chapter <= upto and chapter >= chosen.get(key, (0, None))[0]:
            chosen[key] = (chapter, index, marker)

    starts = {index for _chapter, index, _marker in chosen.values()}
    bounds = [index for index, _, _, _ in blocks] + [len(lines)]
    for position, (index, _chapter, _key, marker) in enumerate(blocks):
        if index not in starts:
            continue
        for cursor in range(index + 1, bounds[position + 1]):
            lines[cursor] = strip_comment(lines[cursor], marker)
    path.write_text("\n".join(lines) + "\n")


def main() -> int:
    project = pathlib.Path(sys.argv[1])
    upto = int(sys.argv[2])
    agentic = len(sys.argv) > 3 and sys.argv[3] == "agentic"
    for name in YAML_FILES:
        enable_yaml(project / name, upto, agentic)
    for source in SOURCES:
        enable_source(project / source, upto)
    return 0


if __name__ == "__main__":
    sys.exit(main())
