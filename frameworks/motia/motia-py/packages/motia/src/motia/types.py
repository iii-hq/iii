"""Type definitions for Motia framework."""

from __future__ import annotations

import inspect
from typing import Any, Awaitable, Callable, Generic, Literal, Protocol, TypeVar

from pydantic import BaseModel, ConfigDict, Field

_list = list  # module-level alias; InternalStateManager.list() shadows the builtin

TInput = TypeVar("TInput")
TOutput = TypeVar("TOutput")
TEnqueueData = TypeVar("TEnqueueData")
TBody = TypeVar("TBody")
TResult = TypeVar("TResult")
TCommon = TypeVar("TCommon")


class InternalStateManager(Protocol):
    """Protocol for internal state management."""

    async def get(self, scope: str, key: str) -> Any | None: ...
    async def set(self, scope: str, key: str, value: Any) -> Any: ...
    async def update(self, scope: str, key: str, ops: list[dict[str, Any]]) -> Any: ...
    async def delete(self, scope: str, key: str) -> Any | None: ...
    async def list(self, scope: str) -> _list[Any]: ...
    async def clear(self, scope: str) -> None: ...
    async def list_groups(self) -> _list[str]: ...


Enqueuer = Callable[[Any], Awaitable[None]]


class FlowContext(BaseModel, Generic[TEnqueueData]):
    """Context passed to step handlers."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    trace_id: str
    trigger: "TriggerInfo"
    input_value: Any = None

    def is_queue(self) -> bool:
        """Return True if the trigger is a queue trigger."""
        return self.trigger.type == "queue"

    def is_api(self) -> bool:
        """Return True if the trigger is an API request."""
        return self.trigger.type == "http"

    def is_cron(self) -> bool:
        """Return True if the trigger is a cron event."""
        return self.trigger.type == "cron"

    def is_state(self) -> bool:
        """Return True if the trigger is a state change."""
        return self.trigger.type == "state"

    def is_stream(self) -> bool:
        """Return True if the trigger is a stream event."""
        return self.trigger.type == "stream"

    def get_data(self) -> Any:
        """Extract the data payload from the input, regardless of trigger type.

        - For HTTP triggers: returns request.body
        - For queue triggers: returns the queue data directly
        - For cron triggers: returns None
        """
        if self.is_cron():
            return None
        if isinstance(self.input_value, MotiaHttpArgs):
            return self.input_value.request.body
        if isinstance(self.input_value, ApiRequest):
            return self.input_value.body
        return self.input_value

    async def match(self, handlers: dict[str, Any]) -> Any:
        """Match handlers based on trigger type.

        Handler signatures (matching JS SDK):
        - queue: async (input) -> None
        - http: async (request) -> ApiResponse
        - cron: async () -> None
        - state: async (input) -> Any
        - stream: async (input) -> Any
        - default: async (input) -> Any
        """

        async def _call(handler: Callable[..., Any], *args: Any) -> Any:
            result = handler(*args)
            if inspect.iscoroutine(result):
                return await result
            return result

        checks = [
            (self.is_queue, ["queue"], True),
            (self.is_api, ["http", "api"], True),
            (self.is_cron, ["cron"], False),
            (self.is_state, ["state"], True),
            (self.is_stream, ["stream"], True),
        ]

        for check, keys, has_input in checks:
            if check():
                handler = next((handlers[k] for k in keys if k in handlers), None)
                if handler:
                    return await _call(handler, self.input_value) if has_input else await _call(handler)

        if handlers.get("default"):
            return await _call(handlers["default"], self.input_value)

        raise RuntimeError(
            f"No handler matched for trigger type: {self.trigger.type}. "
            f"Available handlers: {', '.join(k for k in handlers if k != 'default')}"
        )


class Enqueue(BaseModel):
    """Enqueue configuration."""

    topic: str
    label: str | None = None
    conditional: bool = False


class HandlerConfig(BaseModel):
    """Handler infrastructure configuration."""

    ram: int = 128
    cpu: int | None = None
    timeout: int = 30


class QueueConfig(BaseModel):
    """Queue configuration."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["fifo", "standard"] = "standard"
    max_retries: int = Field(default=3, serialization_alias="maxRetries")
    visibility_timeout: int = Field(default=30, serialization_alias="visibilityTimeout")
    delay_seconds: int = Field(default=0, serialization_alias="delaySeconds")
    concurrency: int | None = Field(default=None)
    backoff_type: str | None = Field(default=None, serialization_alias="backoffType")
    backoff_delay_ms: int | None = Field(default=None, serialization_alias="backoffDelayMs")


ApiRouteMethod = Literal["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"]


class TriggerInfo(BaseModel):
    """Information about the trigger that fired."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["http", "queue", "cron", "state", "stream"]
    index: int | None = None

    # API trigger specific
    path: str | None = None
    method: str | None = None

    # Queue trigger specific
    topic: str | None = None

    # Cron trigger specific
    expression: str | None = None


TriggerInput = Any

TriggerCondition = Callable[[Any, FlowContext[Any]], bool | Awaitable[bool]]


class QueueTrigger(BaseModel):
    """Queue trigger configuration."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["queue"] = "queue"
    topic: str
    condition: TriggerCondition | None = None
    input: Any | None = None
    config: QueueConfig | None = None


class QueryParam(BaseModel):
    """Query parameter definition."""

    name: str
    description: str


class ApiTrigger(BaseModel):
    """API trigger configuration."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["http"] = "http"
    path: str
    method: ApiRouteMethod
    condition: TriggerCondition | None = None
    middleware: list[Any] | None = None  # ApiMiddleware
    body_schema: Any | None = Field(default=None, serialization_alias="bodySchema")
    response_schema: dict[int, Any] | None = Field(default=None, serialization_alias="responseSchema")
    query_params: list[QueryParam] | None = Field(default=None, serialization_alias="queryParams")


class CronTrigger(BaseModel):
    """Cron trigger configuration."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["cron"] = "cron"
    expression: str
    condition: TriggerCondition | None = None


class StateTriggerInput(BaseModel):
    """Input received when a state trigger fires."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["state"] = "state"
    group_id: str = Field(alias="groupId")
    item_id: str = Field(alias="itemId")
    old_value: Any | None = Field(default=None, alias="oldValue")
    new_value: Any | None = Field(default=None, alias="newValue")


class StateTrigger(BaseModel):
    """Trigger that fires when state changes."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["state"] = "state"
    condition: TriggerCondition | None = None


def state(*, condition: TriggerCondition | None = None) -> StateTrigger:
    """Create a state trigger."""
    return StateTrigger(type="state", condition=condition)


class StreamEvent(BaseModel):
    """Event data from a stream operation."""

    type: Literal["create", "update", "delete"]
    data: Any


class StreamTriggerInput(BaseModel):
    """Input received when a stream trigger fires."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["stream"] = "stream"
    timestamp: int
    stream_name: str = Field(alias="streamName")
    group_id: str = Field(alias="groupId")
    id: str
    event: StreamEvent


class StreamTrigger(BaseModel):
    """Trigger that fires when stream events occur."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    type: Literal["stream"] = "stream"
    stream_name: str = Field(alias="streamName")
    group_id: str | None = Field(default=None, alias="groupId")
    item_id: str | None = Field(default=None, alias="itemId")
    condition: TriggerCondition | None = None


def stream(
    stream_name: str,
    *,
    group_id: str | None = None,
    item_id: str | None = None,
    condition: TriggerCondition | None = None,
) -> StreamTrigger:
    """Create a stream trigger."""
    return StreamTrigger(
        type="stream",
        stream_name=stream_name,
        group_id=group_id,
        item_id=item_id,
        condition=condition,
    )


TriggerConfig = QueueTrigger | ApiTrigger | CronTrigger | StateTrigger | StreamTrigger


class ApiRequest(BaseModel, Generic[TBody]):
    """API request object."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    path_params: dict[str, str] = Field(default_factory=dict, serialization_alias="pathParams")
    query_params: dict[str, str | list[str]] = Field(default_factory=dict, serialization_alias="queryParams")
    body: TBody | None = None
    headers: dict[str, str | list[str]] = Field(default_factory=dict)


class ApiResponse(BaseModel, Generic[TOutput]):
    """API response object."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    status: int
    body: Any
    headers: dict[str, str] = Field(default_factory=dict)


class MotiaHttpResponse:
    """Streaming HTTP response for channel-based API triggers."""

    def __init__(self, writer: Any) -> None:
        self._writer = writer

    async def status(self, status_code: int) -> None:
        import json

        if self._writer is not None:
            await self._writer.send_message_async(json.dumps({"type": "set_status", "status_code": status_code}))

    async def headers(self, headers: dict[str, str]) -> None:
        import json

        if self._writer is not None:
            await self._writer.send_message_async(json.dumps({"type": "set_headers", "headers": headers}))

    @property
    def writer(self) -> Any:
        return self._writer

    def close(self) -> None:
        if self._writer is not None:
            self._writer.close()


class MotiaHttpRequest(BaseModel, Generic[TBody]):
    """HTTP request portion of a streaming API trigger."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    path_params: dict[str, str] = Field(default_factory=dict, serialization_alias="pathParams")
    query_params: dict[str, str | list[str]] = Field(default_factory=dict, serialization_alias="queryParams")
    body: TBody | None = None
    headers: dict[str, str | list[str]] = Field(default_factory=dict)
    method: str = ""
    request_body: Any = None  # ChannelReader


class MotiaHttpArgs(BaseModel, Generic[TBody]):
    """Motia HTTP arguments with separate request and response."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    request: MotiaHttpRequest[TBody]
    response: MotiaHttpResponse

    @property
    def path_params(self) -> dict[str, str]:
        """Backward-compatible accessor for request.path_params."""
        return self.request.path_params

    @property
    def query_params(self) -> dict[str, str | list[str]]:
        """Backward-compatible accessor for request.query_params."""
        return self.request.query_params

    @property
    def body(self) -> TBody | None:
        """Backward-compatible accessor for request.body."""
        return self.request.body

    @property
    def headers(self) -> dict[str, str | list[str]]:
        """Backward-compatible accessor for request.headers."""
        return self.request.headers

    @property
    def method(self) -> str:
        """Backward-compatible accessor for request.method."""
        return self.request.method

    @property
    def request_body(self) -> Any:
        """Backward-compatible accessor for request.request_body."""
        return self.request.request_body

    def get(self, key: str, default: Any = None) -> Any:
        """Dict-like access for backward compatibility with handlers expecting dicts."""
        values: dict[str, Any] = {
            "request": self.request,
            "response": self.response,
            "path_params": self.path_params,
            "query_params": self.query_params,
            "body": self.body,
            "headers": self.headers,
            "method": self.method,
            "request_body": self.request_body,
        }
        return values.get(key, default)


ApiMiddleware = Callable[
    [ApiRequest[Any], FlowContext[Any], Callable[[], Awaitable[ApiResponse[Any]]]],
    Awaitable[ApiResponse[Any]],
]


class StepConfig(BaseModel):
    """Configuration for a step with triggers."""

    model_config = ConfigDict(populate_by_name=True)

    name: str
    triggers: list[TriggerConfig]
    enqueues: list[str | Enqueue] = Field(default_factory=list)
    virtual_enqueues: list[str | Enqueue] | None = Field(default=None, serialization_alias="virtualEnqueues")
    virtual_subscribes: list[str] | None = Field(default=None, serialization_alias="virtualSubscribes")
    description: str | None = None
    flows: list[str] | None = None
    include_files: list[str] | None = Field(default=None, serialization_alias="includeFiles")


class Step(BaseModel):
    """Represents a step in a flow."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    file_path: str = Field(serialization_alias="filePath")
    config: StepConfig
