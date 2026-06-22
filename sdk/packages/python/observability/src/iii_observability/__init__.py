"""Deprecated: import from ``iii_helpers.observability`` instead.

This package is a thin re-export shim. All public symbols now live in
``iii_helpers.observability``.
"""

from typing import Any

from iii_helpers.observability import __all__ as _all

__all__ = list(_all)


def __getattr__(name: str) -> Any:
    if name in _all:
        import warnings

        warnings.warn(
            f"Importing {name} from iii_observability is deprecated; "
            f"import it from iii_helpers.observability",
            DeprecationWarning,
            stacklevel=2,
        )
        import iii_helpers.observability as _obs

        return getattr(_obs, name)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
