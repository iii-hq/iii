"""Helper free functions that operate on an :class:`III` client instance.

These were previously instance methods on the SDK client. They take the
``iii`` client as the first argument so the public surface of the client
stays focused on the core lifecycle and registration methods.

Mirrors the Rust ``iii_sdk::helpers`` module and the Node
``iii-sdk/helpers`` subpath export.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Protocol

from .channels import ChannelDirection, ChannelItem
from .stream import IStream
from .triggers import TriggerHandler, TriggerTypeRef
from .types import Channel, IIIClient, extract_channel_refs, is_channel_ref

if TYPE_CHECKING:
    from .iii_types import RegisterTriggerTypeInput

__all__ = [
    "ChannelDirection",
    "ChannelItem",
    "create_channel",
    "create_channel_async",
    "create_stream",
    "extract_channel_refs",
    "is_channel_ref",
    "register_trigger_type",
    "unregister_trigger_type",
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

    def _helpers_register_trigger_type(
        self,
        trigger_type: "RegisterTriggerTypeInput | dict[str, Any]",
        handler: TriggerHandler[Any],
    ) -> TriggerTypeRef[Any, Any]: ...

    def _helpers_unregister_trigger_type(self, id: str) -> None: ...


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


def register_trigger_type(
    iii: IIIClient,
    trigger_type: "RegisterTriggerTypeInput | dict[str, Any]",
    handler: TriggerHandler[Any],
) -> TriggerTypeRef[Any, Any]:
    """Register a custom trigger type with the engine.

    Free-function form of the former ``III.register_trigger_type`` method.
    """
    shim: _IIIWithHelperShims = iii  # type: ignore[assignment]
    return shim._helpers_register_trigger_type(trigger_type, handler)


def unregister_trigger_type(iii: IIIClient, id: str) -> None:
    """Unregister a previously registered trigger type by id.

    Free-function form of the former ``III.unregister_trigger_type``
    method. Takes the trigger type id directly instead of an input object.
    """
    shim: _IIIWithHelperShims = iii  # type: ignore[assignment]
    shim._helpers_unregister_trigger_type(id)
