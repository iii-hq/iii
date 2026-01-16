"""Type definitions for Motia framework."""

from typing import Any, Awaitable, Callable, Generic, Literal, Protocol, TypeVar

from pydantic import BaseModel, ConfigDict, Field

from iii import Logger

from .streams import Stream

TInput = TypeVar("TInput")
TOutput = TypeVar("TOutput")
TEmitData = TypeVar("TEmitData")
TBody = TypeVar("TBody")
TResult = TypeVar("TResult")


class InternalStateManager(Protocol):
    """Protocol for internal state management."""

    async def get(self, group_id: str, key: str) -> Any | None: ...
    async def set(self, group_id: str, key: str, value: Any) -> Any: ...
    async def delete(self, group_id: str, key: str) -> Any | None: ...
    async def get_group(self, group_id: str) -> list[Any]: ...
    async def clear(self, group_id: str) -> None: ...


Emitter = Callable[[Any], Awaitable[None]]


class FlowContext(BaseModel, Generic[TEmitData]):
    """Context passed to step handlers."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    emit: Emitter
    trace_id: str
    state: Any  # InternalStateManager
    logger: Any  # Logger
    streams: dict[str, Stream[Any]] = Field(default_factory=dict)
    trigger: "TriggerMetadata"


EventHandler = Callable[[Any, FlowContext[Any]], Awaitable[None]]


class Emit(BaseModel):
    """Emit configuration."""

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


class InfrastructureConfig(BaseModel):
    """Infrastructure configuration."""

    handler: HandlerConfig | None = None
    queue: QueueConfig | None = None


class EventConfig(BaseModel):
    """Configuration for an event step."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["event"]
    name: str
    subscribes: list[str]
    emits: list[str | Emit]
    description: str | None = None
    virtual_emits: list[str | Emit] | None = Field(default=None, serialization_alias="virtualEmits")
    virtual_subscribes: list[str] | None = Field(default=None, serialization_alias="virtualSubscribes")
    input: Any | None = None
    flows: list[str] | None = None
    include_files: list[str] | None = Field(default=None, serialization_alias="includeFiles")
    infrastructure: InfrastructureConfig | None = None


class NoopConfig(BaseModel):
    """Configuration for a noop step."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["noop"]
    name: str
    virtual_emits: list[str | Emit] = Field(serialization_alias="virtualEmits")
    virtual_subscribes: list[str] = Field(serialization_alias="virtualSubscribes")
    description: str | None = None
    flows: list[str] | None = None


ApiRouteMethod = Literal["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"]


class TriggerMetadata(BaseModel):
    """Metadata about the trigger that fired."""

    type: Literal["api", "event", "cron"]
    index: int | None = None

    path: str | None = None
    method: str | None = None

    topic: str | None = None

    expression: str | None = None


TriggerInput = Any

TriggerCondition = Callable[[Any, FlowContext[Any]], bool | Awaitable[bool]]


class EventTrigger(BaseModel):
    """Event trigger configuration."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["event"] = "event"
    subscribes: list[str]
    condition: TriggerCondition | None = None


class ApiTrigger(BaseModel):
    """API trigger configuration."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["api"] = "api"
    path: str
    method: ApiRouteMethod
    condition: TriggerCondition | None = None


class CronTrigger(BaseModel):
    """Cron trigger configuration."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    type: Literal["cron"] = "cron"
    expression: str
    condition: TriggerCondition | None = None


TriggerConfig = EventTrigger | ApiTrigger | CronTrigger


class QueryParam(BaseModel):
    """Query parameter definition."""

    name: str
    description: str


class ApiRequest(BaseModel, Generic[TBody]):
    """API request object."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    path_params: dict[str, str] = Field(default_factory=dict, serialization_alias="pathParams")
    query_params: dict[str, str | list[str]] = Field(default_factory=dict, serialization_alias="queryParams")
    body: Any | None = None
    headers: dict[str, str | list[str]] = Field(default_factory=dict)


class ApiResponse(BaseModel, Generic[TOutput]):
    """API response object."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    status: int
    body: Any
    headers: dict[str, str] = Field(default_factory=dict)


ApiMiddleware = Callable[
    [ApiRequest[Any], FlowContext[Any], Callable[[], Awaitable[ApiResponse[Any]]]],
    Awaitable[ApiResponse[Any]],
]


class ApiRouteConfig(BaseModel):
    """Configuration for an API route step."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    type: Literal["api"]
    name: str
    path: str
    method: ApiRouteMethod
    emits: list[str | Emit]
    description: str | None = None
    virtual_emits: list[str | Emit] | None = Field(default=None, serialization_alias="virtualEmits")
    virtual_subscribes: list[str] | None = Field(default=None, serialization_alias="virtualSubscribes")
    flows: list[str] | None = None
    middleware: list[Any] | None = None  # ApiMiddleware
    body_schema: Any | None = Field(default=None, serialization_alias="bodySchema")
    response_schema: dict[int, Any] | None = Field(default=None, serialization_alias="responseSchema")
    query_params: list[QueryParam] | None = Field(default=None, serialization_alias="queryParams")
    include_files: list[str] | None = Field(default=None, serialization_alias="includeFiles")


ApiRouteHandler = Callable[[ApiRequest[Any], FlowContext[Any]], Awaitable[ApiResponse[Any]]]


class CronConfig(BaseModel):
    """Configuration for a cron step."""

    model_config = ConfigDict(populate_by_name=True)

    type: Literal["cron"]
    name: str
    cron: str
    emits: list[str | Emit]
    description: str | None = None
    virtual_emits: list[str | Emit] | None = Field(default=None, serialization_alias="virtualEmits")
    virtual_subscribes: list[str] | None = Field(default=None, serialization_alias="virtualSubscribes")
    flows: list[str] | None = None
    include_files: list[str] | None = Field(default=None, serialization_alias="includeFiles")


CronHandler = Callable[[FlowContext[Any]], Awaitable[None]]


class StepConfig(BaseModel):
    """Configuration for a step with triggers."""

    model_config = ConfigDict(populate_by_name=True)

    name: str
    triggers: list[TriggerConfig]
    emits: list[str | Emit] = Field(default_factory=list)
    description: str | None = None
    flows: list[str] | None = None
    include_files: list[str] | None = Field(default=None, serialization_alias="includeFiles")
    infrastructure: InfrastructureConfig | None = None


class Step(BaseModel, Generic[TInput]):
    """Represents a step in a flow."""

    model_config = ConfigDict(arbitrary_types_allowed=True, populate_by_name=True)

    file_path: str = Field(serialization_alias="filePath")
    version: str
    config: StepConfig
