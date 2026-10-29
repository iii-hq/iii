"""Type definitions for the III SDK."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import (
    TYPE_CHECKING,
    Any,
    Awaitable,
    Callable,
    Generic,
    Literal,
    Protocol,
    TypedDict,
    TypeGuard,
    TypeVar,
)

from iii_helpers.http import HttpInvocationConfig
from pydantic import BaseModel, ConfigDict

from . import _wire_json
from .iii_constants import ConnectionStateCallback
from .iii_types import CallRequest, RegisterFunctionMessage, StreamChannelRef

if TYPE_CHECKING:
    from .channels import ChannelReader, ChannelWriter, WritableStream

TInput = TypeVar("TInput")
TOutput = TypeVar("TOutput")


RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]


class RemoteFunctionData(BaseModel):
    """Data for a remote function."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    message: RegisterFunctionMessage
    handler: RemoteFunctionHandler | None = None
    #: Observers of this function as the engine pushed them, local dispatch
    #: state and the deliver hook (an ``iii.observable.FunctionObservers``).
    observe: Any = None
    #: A sync handler, called in-process on a thread of its own by a local
    #: ``call()`` (``None`` for async handlers).
    call_sync: Callable[[Any, Any], Any] | None = None
    #: Holds back ``registerfunction`` until the engine advertised ``observe.v2``.
    needs_v2: bool = False
    #: Retries a ``FUNCTION_NAMESPACE_CONFLICT`` until the id is free.
    standby: bool = False


class Invocation(Generic[TOutput]):
    """Represents an invocation that can be resolved or rejected."""

    def __init__(self, future: asyncio.Future[TOutput]) -> None:
        self._future = future

    def resolve(self, value: TOutput) -> None:
        """Resolve the invocation with a value."""
        if not self._future.done():
            self._future.set_result(value)

    def reject(self, error: Exception) -> None:
        """Reject the invocation with an error."""
        if not self._future.done():
            self._future.set_exception(error)


class RemoteServiceFunctionData(BaseModel):
    """Data for a remote service function."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    message: RegisterFunctionMessage
    handler: RemoteFunctionHandler


class IIIClient(Protocol):
    """Protocol for III client implementations.

    Helper free functions live in :mod:`iii.helpers`. See
    :func:`iii.helpers.create_channel` and :func:`iii.helpers.create_stream`.
    """

    def register_function(
        self,
        function_id: str,
        handler_or_invocation: RemoteFunctionHandler | HttpInvocationConfig,
    ) -> Any: ...

    def call(self, request: dict[str, Any] | CallRequest) -> Any: ...

    async def call_async(self, request: dict[str, Any] | CallRequest) -> Any: ...

    def trigger(self, request: dict[str, Any] | CallRequest) -> Any: ...

    def observe(self, source: Any, target: Any, options: Any = None) -> Any: ...

    def on_observer_added(self, function_id_or_hook: Any, hook: Any = ...) -> Any: ...

    def on_observer_removed(self, function_id_or_hook: Any, hook: Any = ...) -> Any: ...

    def on_observer_deliver(self, function_id: str, hook: Any = ...) -> Any: ...

    def observers(self, function_id: str) -> list[Any]: ...

    def is_observed(self, function_id: str, call: Any = None) -> bool: ...

    def current_invocation(self) -> Any: ...

    def add_connection_state_listener(self, handler: ConnectionStateCallback) -> Callable[[], None]: ...

    def shutdown(self) -> None: ...


@dataclass
class Channel:
    """A streaming channel pair for worker-to-worker data transfer."""

    writer: ChannelWriter
    reader: ChannelReader
    writer_ref: StreamChannelRef
    reader_ref: StreamChannelRef


@dataclass
class InternalHttpRequest:
    """HTTP request with embedded channel references for streaming.

    <!-- docs:internal -->
    """

    path_params: dict[str, str]
    query_params: dict[str, str | list[str]]
    body: Any
    headers: dict[str, str | list[str]]
    method: str
    response: ChannelWriter
    request_body: ChannelReader


class StreamResponse:
    """Streaming response built on top of a ChannelWriter."""

    def __init__(self, writer: ChannelWriter) -> None:
        self._writer = writer

    async def status(self, status_code: int) -> None:
        await self._writer.send_message_async(_wire_json.dumps({"type": "set_status", "status_code": status_code}))

    async def headers(self, headers: dict[str, str]) -> None:
        await self._writer.send_message_async(_wire_json.dumps({"type": "set_headers", "headers": headers}))

    @property
    def stream(self) -> WritableStream:
        return self._writer.stream

    @property
    def writer(self) -> ChannelWriter:
        return self._writer

    def close(self) -> None:
        self._writer.close()


@dataclass
class StreamRequest:
    """Incoming streaming request received by a function that serves a streaming HTTP route."""

    path_params: dict[str, str]
    query_params: dict[str, str | list[str]]
    body: Any
    headers: dict[str, str | list[str]]
    method: str
    request_body: ChannelReader


class StreamChannelRefDict(TypedDict):
    """Wire-shape of a :class:`StreamChannelRef` as it appears in raw payloads.

    Used as the narrowed type for :func:`is_channel_ref` so type checkers can
    validate ``value["channel_id"]`` style access after the guard passes.
    """

    channel_id: str
    access_key: str
    direction: Literal["read", "write"]


def is_channel_ref(value: Any) -> TypeGuard[StreamChannelRefDict]:
    """Check if a value looks like a StreamChannelRef.

    Returns a :class:`typing.TypeGuard` that narrows ``value`` to
    :class:`StreamChannelRefDict`, mirroring the TypeScript SDK's
    ``value is StreamChannelRef`` predicate.
    """
    return (
        isinstance(value, dict)
        and isinstance(value.get("channel_id"), str)
        and isinstance(value.get("access_key"), str)
        and value.get("direction") in {"read", "write"}
    )


def extract_channel_refs(data: Any) -> list[tuple[str, StreamChannelRef]]:
    """Extract all channel references from a nested value, returning ``(path, ref)`` tuples.

    Recursively walks ``dict`` and ``list`` structures. Dotted paths are
    used for object fields, bracketed indices for list elements (e.g.
    ``items[0].writer``). Mirrors the Rust SDK's ``extract_channel_refs``.
    """
    refs: list[tuple[str, StreamChannelRef]] = []
    _extract_refs_recursive(data, "", refs)
    return refs


def _extract_refs_recursive(
    data: Any, prefix: str, refs: list[tuple[str, StreamChannelRef]]
) -> None:
    if is_channel_ref(data):
        try:
            refs.append((prefix, StreamChannelRef(**data)))
        except Exception:
            pass
        return
    if isinstance(data, dict):
        for key, value in data.items():
            path = key if not prefix else f"{prefix}.{key}"
            _extract_refs_recursive(value, path, refs)
    elif isinstance(data, list):
        for idx, item in enumerate(data):
            path = f"[{idx}]" if not prefix else f"{prefix}[{idx}]"
            _extract_refs_recursive(item, path, refs)
