"""Type definitions for the III SDK."""

import asyncio
from typing import Any, Awaitable, Callable, Generic, Protocol, TypeVar

from pydantic import BaseModel, ConfigDict, Field

from .bridge_types import RegisterFunctionMessage, RegisterTriggerMessage, RegisterTriggerTypeMessage
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


class BridgeClient(Protocol):
    """Protocol for bridge client implementations."""

    def register_trigger(self, trigger: RegisterTriggerMessage) -> Trigger: ...

    def register_service(self, service_id: str, description: str | None = None) -> None: ...

    def register_function(
        self,
        function_path: str,
        handler: RemoteFunctionHandler,
        description: str | None = None,
    ) -> None: ...

    async def invoke_function(self, function_path: str, data: Any) -> Any: ...

    def invoke_function_async(self, function_path: str, data: Any) -> None: ...

    def register_trigger_type(
        self,
        trigger_type_id: str,
        description: str,
        handler: TriggerHandler[Any],
    ) -> None: ...

    def unregister_trigger_type(self, trigger_type_id: str) -> None: ...


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
