#!/usr/bin/env python3
"""Verify Python README code blocks use only real `III` attributes.

Catches the class of bug where a README teaches `iii.connect()` while the
actual class exposes only `connect_async()`. Compile-only checks (py_compile,
mypy without type stubs) pass such code because `iii.connect()` is valid
Python syntax — it AttributeErrors only at runtime.

Usage:
    python check_python_readme.py README.md [README.md ...]

Exit code 0 = all snippets reference real attributes, 1 = at least one
snippet calls a method that does not exist on `iii.III`.

The script uses `markdown-it-py` to parse code fences correctly (no regex)
and `ast` + `inspect` to resolve attribute references against the real
class.
"""

from __future__ import annotations

import ast
import inspect
import sys
from pathlib import Path

try:
    from markdown_it import MarkdownIt
except ImportError:  # pragma: no cover - bootstrap guidance
    sys.stderr.write(
        "check_python_readme: markdown-it-py is required. "
        "Install with `pip install markdown-it-py`.\n"
    )
    sys.exit(2)


def extract_python_fences(md_text: str) -> list[tuple[int, str]]:
    """Return (line_number, source) for each ```python fenced block."""
    md = MarkdownIt("commonmark")
    tokens = md.parse(md_text)
    fences: list[tuple[int, str]] = []
    for tok in tokens:
        if tok.type != "fence":
            continue
        info = (tok.info or "").strip().lower()
        # Accept `python`, `py`, or info strings that start with `python`
        # (e.g., `python3`, `python,ignore`).
        if info == "python" or info == "py" or info.startswith("python"):
            line = (tok.map[0] + 1) if tok.map else 0
            fences.append((line, tok.content))
    return fences


def iii_class_attrs() -> set[str]:
    """Return the set of public attribute names available on `iii.III`."""
    from iii.iii import III  # noqa: E402 - runtime import

    return {name for name, _ in inspect.getmembers(III) if not name.startswith("_")}


def attr_calls_on_iii_var(source: str) -> list[tuple[int, str]]:
    """Find method calls of the form `iii.<name>(...)` in a snippet.

    Returns a list of (lineno, attr_name). Heuristic: we treat any bare
    identifier named ``iii`` as an `III` instance. The Hello World pattern
    `iii = register_worker(...)` makes this reliable.
    """
    try:
        tree = ast.parse(source)
    except SyntaxError:
        # Syntax errors are a separate class of problem; `py_compile` already
        # catches them. Skip attribute checking when parse fails.
        return []

    calls: list[tuple[int, str]] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        fn = node.func
        if not isinstance(fn, ast.Attribute):
            continue
        if not isinstance(fn.value, ast.Name):
            continue
        if fn.value.id != "iii":
            continue
        calls.append((node.lineno, fn.attr))
    return calls


def check_file(path: Path, allowed_attrs: set[str]) -> list[str]:
    errors: list[str] = []
    md_text = path.read_text()
    for block_line, source in extract_python_fences(md_text):
        for call_lineno, attr in attr_calls_on_iii_var(source):
            if attr in allowed_attrs:
                continue
            # Allow the dynamic `attr` sentinel names we know about:
            # e.g., handler closures aren't on `iii`.
            errors.append(
                f"{path}:{block_line + call_lineno - 1}: "
                f"iii.{attr}() is called but `iii.III` has no public "
                f"attribute named `{attr}`. "
                f"Did the README outlive a refactor?"
            )
    return errors


def main() -> int:
    if len(sys.argv) < 2:
        sys.stderr.write(f"usage: {sys.argv[0]} README.md [README.md ...]\n")
        return 2

    try:
        allowed = iii_class_attrs()
    except Exception as exc:  # pragma: no cover - import failure path
        sys.stderr.write(
            f"check_python_readme: failed to import `iii.III` to probe its "
            f"attributes: {exc}\n"
            "Make sure the iii-sdk package is installed (editable mode is "
            "fine) before running this check.\n"
        )
        return 2

    all_errors: list[str] = []
    for arg in sys.argv[1:]:
        all_errors.extend(check_file(Path(arg), allowed))

    for msg in all_errors:
        sys.stderr.write(msg + "\n")

    return 0 if not all_errors else 1


if __name__ == "__main__":
    sys.exit(main())
