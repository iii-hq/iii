"""Helper free functions that operate on an :class:`III` client instance.

These were previously instance methods on the SDK client. They take the
``iii`` client as the first argument so the public surface of the client
stays focused on the core lifecycle and registration methods.

Mirrors the Rust ``iii_sdk::helpers`` module and the Node
``iii-sdk/helpers`` subpath export.
"""

from __future__ import annotations

import warnings
from typing import Any, Protocol, TypeVar

from .stream import IStream
from .types import IIIClient

TData = TypeVar("TData")

_CREATE_STREAM_DEPRECATION = (
    "create_stream is deprecated (iii-stream) and will be removed in an upcoming release. "
    "Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams"
)

__all__ = [
    "create_stream",
]


class _IIIWithHelperShims(IIIClient, Protocol):
    """Internal Protocol that adds the ``_helpers_*`` shim methods.

    The free functions below delegate to these private methods on the
    concrete :class:`III` instance. Defining the Protocol here mirrors the
    Node SDK's ``IIIWithHelperShims`` intersection type, callers see the
    public :class:`IIIClient` Protocol; helpers see the shims internally.
    """

    def _helpers_create_stream(
        self, stream_name: str, stream: IStream[Any]
    ) -> None: ...  # noqa: D401  (internal shim, generic erased at the boundary)


def create_stream(iii: IIIClient, stream_name: str, stream: IStream[TData]) -> None:
    """Register a custom stream implementation.

    Free-function form of the former ``III.create_stream`` instance method.
    The ``IStream`` generic ``TData`` is preserved so type checkers can
    validate the implementor's get/set/delete/list signatures.

    .. deprecated::
        create_stream is deprecated (iii-stream) and will be removed in an upcoming release.
        Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams

    Emits a :class:`FutureWarning` attributed to the caller's line on every call.
    """
    warnings.warn(_CREATE_STREAM_DEPRECATION, FutureWarning, stacklevel=2)
    shim: _IIIWithHelperShims = iii  # type: ignore[assignment]
    shim._helpers_create_stream(stream_name, stream)
