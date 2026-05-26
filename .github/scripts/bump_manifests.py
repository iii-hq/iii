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
