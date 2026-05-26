#!/usr/bin/env python3
"""Rewrite every release-bumped manifest in lockstep.

Pure functions live at module top so they can be unit-tested. The CLI at
the bottom drives the rewrites the create-tag workflow used to perform
inline with sed/jq.
"""
from __future__ import annotations

import re


_CARGO_PACKAGE_VERSION_RE = re.compile(r'^version = "[^"]*"', re.MULTILINE)


def bump_cargo_package_version(text: str, new_version: str) -> str:
    """Replace the first top-level ``version = "..."`` line in a Cargo manifest."""
    m = _CARGO_PACKAGE_VERSION_RE.search(text)
    if m is None:
        raise ValueError("no top-level version line in Cargo manifest")
    start, end = m.span()
    return f'{text[:start]}version = "{new_version}"{text[end:]}'


def bump_cargo_workspace_dep_version(text: str, dep_name: str, new_version: str) -> str:
    """Replace the ``version = "..."`` value inside a ``dep_name = { ... }`` line.

    Matches a single workspace-dependency entry whose key is exactly
    ``dep_name`` and rewrites the embedded ``version`` field. Other fields
    on the line (``path``, ``features``, ...) are preserved.
    """
    line_re = re.compile(
        rf'^(?P<lead>{re.escape(dep_name)}\s*=\s*\{{[^}}\n]*?version\s*=\s*")[^"]*(?P<tail>"[^}}\n]*\}})',
        re.MULTILINE,
    )
    m = line_re.search(text)
    if m is None:
        raise ValueError(f"no workspace dependency entry for {dep_name!r}")
    return f'{text[:m.start()]}{m.group("lead")}{new_version}{m.group("tail")}{text[m.end():]}'
