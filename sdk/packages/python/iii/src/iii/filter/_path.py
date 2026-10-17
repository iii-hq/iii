"""Filter paths: ``root ("." segment)*`` (filters.md §2)."""

from __future__ import annotations

import re
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from typing import Any

from ._errors import MAX_PATH_SEGMENTS, FilterError
from ._value import ABSENT, array_element, json_view, object_member

#: A path's first segment. ``@`` is the current ``$any`` element.
ROOTS = frozenset({"input", "output", "error", "caller", "metadata", "@"})

_CANONICAL_INDEX = re.compile(r"0|[1-9][0-9]*")


@dataclass(frozen=True)
class Segment:
    """One segment after the root.

    ``index`` is set when the segment is a canonical decimal (``0`` or
    ``[1-9][0-9]*``): it selects an array element, while ``key`` looks up an
    object member.
    """

    key: str
    index: int | None


@dataclass(frozen=True)
class Path:
    root: str
    segments: tuple[Segment, ...]


def _canonical_index(segment: str) -> int | None:
    # Longer than any array can be: never selects an element. ``fullmatch``
    # keeps ``int()``'s leniency (``01``, ``+1``, `` 1``, Unicode digits) out.
    if len(segment) > 15 or _CANONICAL_INDEX.fullmatch(segment) is None:
        return None
    return int(segment)


def _empty_segment(raw: str) -> FilterError:
    return FilterError.invalid("empty path" if raw == "" else f"path `{raw}`: empty segment")


def parse_path(raw: str, in_any: bool) -> Path:
    """Parses a path. ``\\.`` and ``\\\\`` escape inside keys; any other
    backslash sequence, an empty segment or an unknown root is
    ``INVALID_FILTER``. ``@`` is a root only inside ``$any``, where it is the
    only root allowed.
    """
    parts: list[str] = []
    current: list[str] = []
    i = 0
    n = len(raw)
    while i < n:
        c = raw[i]
        if c == "\\":
            nxt = raw[i + 1] if i + 1 < n else None
            if nxt == "." or nxt == "\\":
                current.append(nxt)
                i += 2
                continue
            if nxt is None:
                raise FilterError.invalid(f"path `{raw}`: dangling `\\` at the end")
            raise FilterError.invalid(f"path `{raw}`: invalid escape `\\{nxt}` (only `\\.` and `\\\\` are allowed)")
        if c == ".":
            if not current:
                raise _empty_segment(raw)
            parts.append("".join(current))
            current = []
        else:
            current.append(c)
        i += 1
    if not current:
        raise _empty_segment(raw)
    parts.append("".join(current))

    first = parts[0]
    if first not in ROOTS:
        raise FilterError.invalid(
            f"path `{raw}`: inside `$any` paths start at `@`, the current element"
            if in_any
            else f"path `{raw}`: unknown root `{first}` (expected input, output, error, caller or metadata)"
        )
    if first == "@" and not in_any:
        raise FilterError.invalid(f"path `{raw}`: `@` paths are only allowed inside `$any`")
    if first != "@" and in_any:
        raise FilterError.invalid(f"path `{raw}`: inside `$any` paths start at `@`, the current element")
    segments = tuple(Segment(key, _canonical_index(key)) for key in parts[1:])
    if len(segments) > MAX_PATH_SEGMENTS:
        raise FilterError.invalid(f"path `{raw}`: {len(segments)} segments after the root (limit {MAX_PATH_SEGMENTS})")
    return Path(first, segments)


Resolver = Callable[[Mapping[str, Any], Any], Any]


def compile_path(path: Path) -> Resolver:
    """Compiles the path into a resolver ``(doc, element) -> value``;
    :data:`ABSENT` means absent."""
    segments = path.segments

    def step(start: Any) -> Any:
        current = start
        for segment in segments:
            if isinstance(current, dict):
                current = object_member(current, segment.key)
            elif isinstance(current, (list, tuple)):
                index = segment.index
                if index is None or index >= len(current):
                    return ABSENT
                current = array_element(current, index)
            else:
                return ABSENT
        return current

    if path.root == "@":
        return lambda _doc, element: step(element)

    root = path.root

    def from_doc(doc: Mapping[str, Any], _element: Any) -> Any:
        if root not in doc:
            return ABSENT
        return step(json_view(doc[root]))

    return from_doc
