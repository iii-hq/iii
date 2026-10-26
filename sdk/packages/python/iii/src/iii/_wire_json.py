"""JSON text of an outbound message, written as the other SDKs write it
(``JSON.stringify``, ``serde_json``): compact, with non-ASCII characters as
UTF-8 rather than ``\\u`` escapes. The message limits count the UTF-8 bytes of
this text, so accented, CJK or emoji text counts as many bytes as in those SDKs
(with escapes it took two to three times as many)."""

from __future__ import annotations

import json
import re
from typing import Any

#: A Python string may hold a lone surrogate (``surrogateescape``), which UTF-8
#: cannot encode; JSON can only carry it as a ``\\u`` escape.
_SURROGATES = re.compile("[\ud800-\udfff]")


def _escape(match: re.Match[str]) -> str:
    return f"\\u{ord(match.group()):04x}"


def dumps(value: Any) -> str:
    """Compact UTF-8 JSON text of ``value``. A surrogate code point (only ever
    inside a string) is written as the ``\\u`` escape ``json.dumps`` writes by
    default, so the text always encodes as UTF-8. ``NaN`` and the infinities are
    written as ``json.dumps`` writes them by default."""
    text = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    if text.isascii():
        return text
    try:
        text.encode("utf-8")
    except UnicodeEncodeError:
        return _SURROGATES.sub(_escape, text)
    return text
