"""Type definitions for the III SDK."""

import asyncio
from typing import Any, Awaitable, Callable, Generic, Protocol, TypeVar

from pydantic import BaseModel, ConfigDict, Field

from .bridge_types import FunctionInfo, RegisterFunctionMessage, RegisterTriggerMessage, RegisterTriggerTypeMessage
from .streams import IStream
from .triggers import Trigger, TriggerHandler

TInput = TypeVar("TInput")
TOutput = TypeVar("TOutput")
TConfig = TypeVar("TConfig")


RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]


class RemoteFunctionData(BaseModel):
    """Data for a remote function."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    message: RegisterFunctionMessage
    handler: RemoteFunctionHandler


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
RegisterTriggerInput = RegisterTriggerMessage
RegisterServiceInput = str
RegisterFunctionInput = RegisterFunctionMessage
RegisterTriggerTypeInput = RegisterTriggerTypeMessage


# Callback type for functions available event
FunctionsAvailableCallback = Callable[[list[FunctionInfo]], None]


class BridgeClient(Protocol):
    """Protocol for bridge client implementations."""

    def register_trigger(self, trigger: RegisterTriggerMessage) -> Trigger: ...

    def register_service(self, service_id: str, description: str | None = None) -> None: ...

    def register_function(
        self,
        function_id: str,
        handler: RemoteFunctionHandler,
        description: str | None = None,
    ) -> None: ...

    async def invoke_function(self, function_id: str, data: Any) -> Any: ...

    def invoke_function_async(self, function_id: str, data: Any) -> None: ...

    def register_trigger_type(
        self,
        trigger_type_id: str,
        description: str,
        handler: TriggerHandler[Any],
    ) -> None: ...

    def unregister_trigger_type(self, trigger_type_id: str) -> None: ...

    def on(self, event: str, callback: Callable[..., None]) -> Callable[[], None]: ...

    def create_stream(self, stream_name: str, stream: IStream[Any]) -> None: ...

    def on_functions_available(self, callback: FunctionsAvailableCallback) -> Callable[[], None]: ...


class ApiRequest(BaseModel, Generic[TInput]):
    """Represents an API request."""

    model_config = ConfigDict(populate_by_name=True)

    path_params: dict[str, str] = Field(default_factory=dict, alias="pathParams")
    query_params: dict[str, str | list[str]] = Field(default_factory=dict, alias="queryParams")
    body: Any | None = None
    headers: dict[str, str | list[str]] = Field(default_factory=dict)
    method: str = "GET"


class ApiResponse(BaseModel, Generic[TOutput]):
    """Represents an API response."""

    model_config = ConfigDict(populate_by_name=True, arbitrary_types_allowed=True)

    status_code: int = Field(alias="statusCode")
    body: Any
    headers: dict[str, str] = Field(default_factory=dict)
