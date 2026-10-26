"""The observer filter language (``iii-filter`` v1, filters.md): validation and
evaluation, with the same semantics as the engine's Rust evaluator
(``crates/iii-filter``) and the other SDKs.

Examples:
    >>> from iii.filter import compile_filter
    >>> f = compile_filter({"input.amount": {"$gte": 500}})
    >>> f.matches({"input": {"amount": 750}})
    True
"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from ._compile import compile_source
from ._errors import FilterError, FilterErrorCode

__all__ = ["CompiledFilter", "FilterError", "FilterErrorCode", "MATCH_ALL", "compile_filter"]


class CompiledFilter:
    """A validated filter, compiled once into a closure tree.

    Attributes:
        source: The filter as JSON (what travels on the wire).
        match_all: ``{}``: every document matches.
        roots: The roots any clause references (``input``, ``output``,
            ``error``, ``caller``, ``metadata``).
    """

    __slots__ = ("_pred", "match_all", "roots", "source")

    def __init__(self, src: Any, trusted: bool = False) -> None:
        pred, roots, source, match_all = compile_source(src, trusted)
        self._pred = pred
        self.roots: frozenset[str] = roots
        self.source: dict[str, Any] = source
        self.match_all: bool = match_all

    def matches(self, doc: Mapping[str, Any]) -> bool:
        """Evaluates the filter on a document ``{input?, output?, error?,
        caller?, metadata?}``: a missing key is an absent root, ``None`` the
        value ``null``. Total: never raises for a valid filter, whatever the
        host values hold (an evaluation that fails is a non-match).
        """
        if self.match_all:
            return True
        if not isinstance(doc, Mapping):
            doc = {}
        try:
            return self._pred(doc, None)
        except Exception:
            return False

    def __repr__(self) -> str:
        return f"CompiledFilter({self.source!r})"


def compile_filter(src: Any, *, trusted: bool = False) -> CompiledFilter:
    """Validates and compiles a filter.

    ``trusted``: the filter was validated by the engine (the normal form it
    pushes to a function's owner); skip the encoded size limit, which the
    engine already applied to the filter as written.

    Raises:
        FilterError: ``INVALID_FILTER`` when the filter is not valid.
    """
    return CompiledFilter(src, trusted)


#: The filter that matches every document (``{}``).
MATCH_ALL = CompiledFilter({})
