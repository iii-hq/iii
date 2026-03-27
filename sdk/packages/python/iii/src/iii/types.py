"""Type definitions for the III SDK."""

from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Awaitable, Callable, Generic, Protocol, TypeVar

from pydantic import BaseModel, ConfigDict, Field

from .iii_types import (
    FunctionInfo,
    HttpInvocationConfig,
    RegisterFunctionInput,
    RegisterFunctionMessage,
    RegisterServiceInput,
    RegisterTriggerInput,
    RegisterTriggerTypeInput,
    RegisterTriggerTypeMessage,
    StreamChannelRef,
    TriggerInfo,
    TriggerRequest,
)
from .stream import IStream
from .triggers import Trigger, TriggerHandler

if TYPE_CHECKING:
    from .channels import ChannelReader, ChannelWriter, WritableStream

TInput = TypeVar("TInput")
TOutput = TypeVar("TOutput")
TConfig = TypeVar("TConfig")


RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]


class RemoteFunctionData(BaseModel):
    """Data for a remote function."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    message: RegisterFunctionMessage
    handler: RemoteFunctionHandler | None = None


class RemoteTriggerTypeData(BaseModel):
    """Data for a remote trigger type."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    message: RegisterTriggerTypeMessage
    handler: TriggerHandler[Any]


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


# Type aliases for registration inputs
# Callback type for functions available event
FunctionsAvailableCallback = Callable[[list[FunctionInfo]], None]


class IIIClient(Protocol):
    """Protocol for III client implementations."""

    def register_trigger(self, trigger: RegisterTriggerInput | dict[str, Any]) -> Trigger: ...

    def register_service(self, service: RegisterServiceInput | dict[str, Any]) -> None: ...

    def register_function(
        self,
        func: RegisterFunctionInput | dict[str, Any],
        handler_or_invocation: RemoteFunctionHandler | HttpInvocationConfig,
    ) -> Any: ...

    def trigger(self, request: dict[str, Any] | TriggerRequest) -> Any: ...

    def register_trigger_type(
        self,
        trigger_type: RegisterTriggerTypeInput | dict[str, Any],
        handler: TriggerHandler[Any],
    ) -> Any: ...

    def unregister_trigger_type(self, trigger_type: RegisterTriggerTypeInput | dict[str, Any]) -> None: ...

    def list_trigger_types(self, include_internal: bool = False) -> list[Any]: ...

    def create_channel(self, buffer_size: int | None = None) -> Channel: ...

    def create_stream(self, stream_name: str, stream: IStream[Any]) -> None: ...

    def list_functions(self) -> list[FunctionInfo]: ...

    def list_triggers(self, include_internal: bool = False) -> list[TriggerInfo]: ...

    def on_functions_available(self, callback: FunctionsAvailableCallback) -> Callable[[], None]: ...

    def shutdown(self) -> None: ...


class ApiRequest(BaseModel, Generic[TInput]):
    """Represents an API request."""

    path_params: dict[str, str] = Field(default_factory=dict)
    query_params: dict[str, str | list[str]] = Field(default_factory=dict)
    body: Any | None = None
    headers: dict[str, str | list[str]] = Field(default_factory=dict)
    method: str = "GET"


class ApiResponse(BaseModel, Generic[TOutput]):
    """Represents an API response."""

    model_config = ConfigDict(populate_by_name=True, arbitrary_types_allowed=True)

    status_code: int = Field(alias="statusCode")
    body: Any | None = None
    headers: dict[str, str] = Field(default_factory=dict)


@dataclass
class Channel:
    """A streaming channel pair for worker-to-worker data transfer."""

    writer: ChannelWriter
    reader: ChannelReader
    writer_ref: StreamChannelRef
    reader_ref: StreamChannelRef


@dataclass
class InternalHttpRequest:
    """HTTP request with embedded channel references for streaming."""

    path_params: dict[str, str]
    query_params: dict[str, str | list[str]]
    body: Any
    headers: dict[str, str | list[str]]
    method: str
    response: ChannelWriter
    request_body: ChannelReader


class HttpResponse:
    """Streaming HTTP response built on top of a ChannelWriter."""

    def __init__(self, writer: ChannelWriter) -> None:
        self._writer = writer

    async def status(self, status_code: int) -> None:
        await self._writer.send_message_async(json.dumps({"type": "set_status", "status_code": status_code}))

    async def headers(self, headers: dict[str, str]) -> None:
        await self._writer.send_message_async(json.dumps({"type": "set_headers", "headers": headers}))

    @property
    def stream(self) -> WritableStream:
        return self._writer.stream

    @property
    def writer(self) -> ChannelWriter:
        return self._writer

    def close(self) -> None:
        self._writer.close()


@dataclass
class HttpRequest:
    """HTTP request without the response writer."""

    path_params: dict[str, str]
    query_params: dict[str, str | list[str]]
    body: Any
    headers: dict[str, str | list[str]]
    method: str
    request_body: ChannelReader


def is_channel_ref(value: Any) -> bool:
    """Check if a value looks like a StreamChannelRef."""
    return (
        isinstance(value, dict)
        and isinstance(value.get("channel_id"), str)
        and isinstance(value.get("access_key"), str)
        and value.get("direction") in {"read", "write"}
    )
