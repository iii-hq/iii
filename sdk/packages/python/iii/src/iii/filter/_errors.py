"""Errors and structural limits of the filter language (filters.md §4)."""

from __future__ import annotations

from typing import Literal

#: Nesting depth of filters (the top-level filter is at depth 1).
MAX_DEPTH = 8
#: Operator / clause nodes (each value operator, ``$or`` and ``$not`` counts one).
MAX_NODES = 64
#: Items of one ``$in`` / ``$nin`` array.
MAX_SET_ITEMS = 256
#: Path segments after the root.
MAX_PATH_SEGMENTS = 16
#: UTF-8 bytes of the compact JSON encoding of the filter.
MAX_ENCODED_BYTES = 8192
#: Numeric literals in a filter must satisfy ``|n| <= 2^53``.
MAX_SAFE_LITERAL = 2**53
#: Arrays / objects nested in one ``$eq`` / ``$ne`` operand or one ``$in`` / ``$nin`` item.
MAX_LITERAL_DEPTH = 32
#: Arrays / objects nested in the whole filter value (the top-level object is 1).
MAX_JSON_DEPTH = 120

FilterErrorCode = Literal["INVALID_FILTER", "INVALID_FILTER_PATH"]


class FilterError(ValueError):
    """A filter the language rejects.

    ``code`` is the protocol error code (protocol.md §4): ``INVALID_FILTER``
    for every rule this evaluator checks.
    """

    def __init__(self, code: FilterErrorCode, message: str) -> None:
        # Both in `args`, so the error pickles (and unpickles) whole.
        super().__init__(code, message)
        self.code: FilterErrorCode = code
        self.message = message

    def __str__(self) -> str:
        return self.message

    @classmethod
    def invalid(cls, message: str) -> FilterError:
        return cls("INVALID_FILTER", message)
