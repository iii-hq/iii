"""Helper free functions that operate on an :class:`III` client instance.

These were previously instance methods on the SDK client. They take the
``iii`` client as the first argument so the public surface of the client
stays focused on the core lifecycle and registration methods.

Mirrors the Rust ``iii_sdk::helpers`` module and the Node
``iii-sdk/helpers`` subpath export.
"""

from __future__ import annotations

from typing import Any, Protocol

from .channels import ChannelDirection, ChannelItem
from .stream import IStream
from .types import Channel, IIIClient, extract_channel_refs, is_channel_ref

__all__ = [
    "ChannelDirection",
    "ChannelItem",
    "create_channel",
    "create_channel_async",
    "create_stream",
    "extract_channel_refs",
    "is_channel_ref",
]


class _IIIWithHelperShims(IIIClient, Protocol):
    """Internal Protocol that adds the ``_helpers_*`` shim methods.

    The free functions below delegate to these private methods on the
    concrete :class:`III` instance. Defining the Protocol here mirrors the
    Node SDK's ``IIIWithHelperShims`` intersection type — callers see the
    public :class:`IIIClient` Protocol; helpers see the shims internally.
    """

    def _helpers_create_channel(self, buffer_size: int | None = None) -> Channel: ...

    async def _helpers_create_channel_async(
        self, buffer_size: int | None = None
    ) -> Channel: ...

    def _helpers_create_stream(
        self, stream_name: str, stream: IStream[Any]
    ) -> None: ...


def create_channel(iii: IIIClient, buffer_size: int | None = None) -> Channel:
    """Create a streaming channel pair (sync wrapper).

    Free-function form of the former ``III.create_channel`` instance method.
    """
    shim: _IIIWithHelperShims = iii  # type: ignore[assignment]
    return shim._helpers_create_channel(buffer_size)


async def create_channel_async(
    iii: IIIClient, buffer_size: int | None = None
) -> Channel:
    """Create a streaming channel pair (async).

    Free-function form of the former ``III.create_channel_async`` method.
    """
    shim: _IIIWithHelperShims = iii  # type: ignore[assignment]
    return await shim._helpers_create_channel_async(buffer_size)


def create_stream(iii: IIIClient, stream_name: str, stream: IStream[Any]) -> None:
    """Register a custom stream implementation.

    Free-function form of the former ``III.create_stream`` instance method.
    """
    shim: _IIIWithHelperShims = iii  # type: ignore[assignment]
    shim._helpers_create_stream(stream_name, stream)
