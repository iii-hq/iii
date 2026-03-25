"""III message types."""

from enum import Enum
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field


class HttpAuthHmac(BaseModel):
    """HMAC signature verification using a shared secret."""

    type: Literal["hmac"] = "hmac"
    secret_key: str = Field(description="Environment variable name containing the HMAC shared secret.")


class HttpAuthBearer(BaseModel):
    """Bearer token authentication."""

    type: Literal["bearer"] = "bearer"
    token_key: str = Field(description="Environment variable name containing the bearer token.")


class HttpAuthApiKey(BaseModel):
    """API key sent via a custom header."""

    type: Literal["api_key"] = "api_key"
    header: str = Field(description="HTTP header name for the API key.")
    value_key: str = Field(description="Environment variable name containing the API key value.")


HttpAuthConfig = HttpAuthHmac | HttpAuthBearer | HttpAuthApiKey
"""Authentication configuration for HTTP-invoked functions."""


class HttpInvocationConfig(BaseModel):
    """Config for HTTP external function invocation.

    Attributes:
        url: Target URL for the HTTP invocation.
        method: HTTP method. Defaults to ``'POST'``.
        timeout_ms: Request timeout in milliseconds.
        headers: Additional HTTP headers to include in the request.
        auth: Authentication configuration (bearer, HMAC, or API key).
    """

    url: str = Field(description="Target URL for the HTTP invocation.")
    method: Literal["GET", "POST", "PUT", "PATCH", "DELETE"] = Field(
        default="POST", description="HTTP method. Defaults to ``'POST'``."
    )
    timeout_ms: int | None = Field(default=None, description="Request timeout in milliseconds.")
    headers: dict[str, str] | None = Field(
        default=None,
        description="Additional HTTP headers to include in the request.",
    )
    auth: HttpAuthConfig | None = Field(
        default=None,
        description="Authentication configuration (bearer, HMAC, or API key).",
    )


class MessageType(str, Enum):
    """Message types for iii communication."""

    REGISTER_FUNCTION = "registerfunction"
    UNREGISTER_FUNCTION = "unregisterfunction"
    REGISTER_SERVICE = "registerservice"
    INVOKE_FUNCTION = "invokefunction"
    INVOCATION_RESULT = "invocationresult"
    REGISTER_TRIGGER_TYPE = "registertriggertype"
    REGISTER_TRIGGER = "registertrigger"
    UNREGISTER_TRIGGER = "unregistertrigger"
    UNREGISTER_TRIGGER_TYPE = "unregistertriggertype"
    TRIGGER_REGISTRATION_RESULT = "triggerregistrationresult"
    WORKER_REGISTERED = "workerregistered"


class RegisterTriggerTypeMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    description: str
    trigger_request_format: Any | None = Field(default=None)
    call_request_format: Any | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.REGISTER_TRIGGER_TYPE, alias="type")


class UnregisterTriggerTypeMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    message_type: MessageType = Field(default=MessageType.UNREGISTER_TRIGGER_TYPE, alias="type")


class UnregisterTriggerMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    message_type: MessageType = Field(default=MessageType.UNREGISTER_TRIGGER, alias="type")
    trigger_type: str | None = Field(default=None, alias="trigger_type")


class TriggerRegistrationResultMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    trigger_type: str = Field(alias="trigger_type")
    function_id: str = Field()
    result: Any = None
    error: Any = None
    message_type: MessageType = Field(default=MessageType.TRIGGER_REGISTRATION_RESULT, alias="type")


class RegisterTriggerTypeInput(BaseModel):
    """Input for registering a trigger type.

    Attributes:
        id: Unique identifier for the trigger type.
        description: Human-readable description of the trigger type.
        trigger_request_format: JSON Schema describing the expected trigger config.
        call_request_format: JSON Schema describing the payload sent to functions.
    """

    id: str = Field(description="Unique identifier for the trigger type.")
    description: str = Field(description="Human-readable description of the trigger type.")
    trigger_request_format: Any | None = Field(
        default=None, description="JSON Schema for trigger configuration."
    )
    call_request_format: Any | None = Field(
        default=None, description="JSON Schema for the call request payload."
    )


class RegisterTriggerInput(BaseModel):
    """Input for registering a trigger (matches Node SDK's RegisterTriggerInput).

    Attributes:
        type: Trigger type identifier (e.g. ``http``, ``queue``, ``cron``).
        function_id: ID of the function this trigger invokes.
        config: Trigger-type-specific configuration.
    """

    type: str = Field(description="Trigger type identifier.")
    function_id: str = Field(description="ID of the function this trigger invokes.")
    config: Any = Field(default=None, description="Trigger-type-specific configuration.")


class RegisterServiceInput(BaseModel):
    """Input for registering a service (matches Node SDK's RegisterServiceInput).

    Attributes:
        id: Unique service identifier.
        name: Human-readable service name.
        description: Description of the service.
        parent_service_id: ID of the parent service for hierarchical grouping.
    """

    id: str = Field(description="Unique service identifier.")
    name: str | None = Field(default=None, description="Human-readable service name.")
    description: str | None = Field(default=None, description="Description of the service.")
    parent_service_id: str | None = Field(
        default=None,
        description="ID of the parent service for hierarchical grouping.",
    )


class RegisterTriggerMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    trigger_type: str = Field(alias="trigger_type")
    function_id: str = Field()
    config: Any
    message_type: MessageType = Field(default=MessageType.REGISTER_TRIGGER, alias="type")


class RegisterServiceMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    name: str | None = None
    description: str | None = None
    parent_service_id: str | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.REGISTER_SERVICE, alias="type")


class RegisterFunctionFormat(BaseModel):
    """Format definition for function parameters.

    Attributes:
        name: Parameter name.
        type: Type string (``string``, ``number``, ``boolean``, ``object``, ``array``, ``null``, ``map``).
        description: Human-readable description of the parameter.
        body: Nested fields for object types.
        items: Item schema for array types.
        required: Whether the parameter is required.
    """

    name: str
    type: str  # 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map'
    description: str | None = None
    body: list["RegisterFunctionFormat"] | None = None
    items: "RegisterFunctionFormat | None" = None
    required: bool = False


class RegisterFunctionInput(BaseModel):
    """Input for registering a function — matches Node.js RegisterFunctionInput.

    Attributes:
        id: Unique function identifier.
        description: Human-readable description.
        request_format: Schema describing expected input.
        response_format: Schema describing expected output.
        metadata: Arbitrary metadata attached to the function.
        invocation: HTTP invocation config for externally hosted functions.
    """

    id: str = Field(description="Unique function identifier.")
    description: str | None = Field(default=None, description="Human-readable description.")
    request_format: RegisterFunctionFormat | dict[str, Any] | None = Field(
        default=None, description="Schema describing expected input."
    )
    response_format: RegisterFunctionFormat | dict[str, Any] | None = Field(
        default=None, description="Schema describing expected output."
    )
    metadata: dict[str, Any] | None = Field(default=None, description="Arbitrary metadata attached to the function.")
    invocation: HttpInvocationConfig | None = Field(
        default=None,
        description="HTTP invocation config for externally hosted functions.",
    )


class RegisterFunctionMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str = Field()
    description: str | None = None
    request_format: RegisterFunctionFormat | dict[str, Any] | None = Field(default=None)
    response_format: RegisterFunctionFormat | dict[str, Any] | None = Field(default=None)
    metadata: dict[str, Any] | None = None
    invocation: HttpInvocationConfig | None = None
    message_type: MessageType = Field(default=MessageType.REGISTER_FUNCTION, alias="type")


class TriggerActionEnqueue(BaseModel):
    """Routes the invocation through a named queue for async processing.

    Attributes:
        type: Always ``'enqueue'``.
        queue: Name of the target queue.
    """

    type: Literal["enqueue"] = "enqueue"
    queue: str


class TriggerActionVoid(BaseModel):
    """Fire-and-forget routing. No response is returned.

    Attributes:
        type: Always ``'void'``.
    """

    type: Literal["void"] = "void"


TriggerAction = TriggerActionEnqueue | TriggerActionVoid
"""Routing action for trigger requests."""


class EnqueueResult(BaseModel):
    """Result returned when a function is invoked with ``TriggerAction.Enqueue``.

    Attributes:
        messageReceiptId: UUID assigned by the engine to the enqueued job.
    """

    messageReceiptId: str = Field(description="UUID assigned by the engine to the enqueued job.")


class TriggerRequest(BaseModel):
    """Request object for ``trigger()``.

    Attributes:
        function_id: ID of the function to invoke.
        payload: Data to pass to the function.
        action: Routing action — ``None`` for sync, ``TriggerAction.Enqueue(...)``
            for queue, ``TriggerAction.Void()`` for fire-and-forget.
        timeout_ms: Override the default invocation timeout.
    """

    function_id: str = Field(description="ID of the function to invoke.")
    payload: Any = Field(default=None, description="Data to pass to the function.")
    action: TriggerActionEnqueue | TriggerActionVoid | None = Field(
        default=None,
        description=(
            "Routing action: ``None`` for sync, "
            "``TriggerAction.Enqueue(...)`` for queue, "
            "``TriggerAction.Void()`` for fire-and-forget."
        ),
    )
    timeout_ms: int | None = Field(default=None, description="Override the default invocation timeout.")


class InvokeFunctionMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    function_id: str = Field()
    data: Any
    invocation_id: str | None = Field(default=None)
    traceparent: str | None = Field(default=None)
    baggage: str | None = Field(default=None)
    action: TriggerActionEnqueue | TriggerActionVoid | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.INVOKE_FUNCTION, alias="type")


class InvocationResultMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    invocation_id: str = Field()
    function_id: str = Field()
    result: Any = None
    error: Any = None
    traceparent: str | None = Field(default=None)
    baggage: str | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.INVOCATION_RESULT, alias="type")


class WorkerRegisteredMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    worker_id: str = Field()
    message_type: MessageType = Field(default=MessageType.WORKER_REGISTERED, alias="type")


class UnregisterFunctionMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    message_type: MessageType = Field(default=MessageType.UNREGISTER_FUNCTION, alias="type")


class FunctionInfo(BaseModel):
    """Information about a registered function.

    Attributes:
        function_id: Unique identifier of the function.
        description: Human-readable description.
        request_format: Schema describing expected input (JSON Schema or custom format).
        response_format: Schema describing expected output (JSON Schema or custom format).
        metadata: Arbitrary metadata attached to the function.
    """

    function_id: str = Field(description="Unique identifier of the function.")
    description: str | None = Field(default=None, description="Human-readable description.")
    request_format: dict[str, Any] | None = Field(
        default=None, description="Schema describing expected input (JSON Schema or custom format)."
    )
    response_format: dict[str, Any] | None = Field(
        default=None, description="Schema describing expected output (JSON Schema or custom format)."
    )
    metadata: dict[str, Any] | None = Field(
        default=None, description="Arbitrary metadata attached to the function."
    )


class TriggerInfo(BaseModel):
    """Information about a registered trigger.

    Attributes:
        id: Unique trigger identifier.
        trigger_type: Type of trigger (e.g. ``http``, ``queue``, ``cron``).
        function_id: ID of the function this trigger invokes.
        config: Trigger-type-specific configuration.
    """

    id: str = Field(description="Unique trigger identifier.")
    trigger_type: str = Field(description="Type of trigger (e.g. ``http``, ``queue``, ``cron``).")
    function_id: str = Field(description="ID of the function this trigger invokes.")
    config: Any = Field(default=None, description="Trigger-type-specific configuration.")


class TriggerTypeInfo(BaseModel):
    """Information about a registered trigger type.

    Attributes:
        id: Trigger type identifier (e.g. ``http``, ``cron``, ``queue``).
        description: Human-readable description of the trigger type.
        trigger_request_format: JSON Schema for the trigger configuration.
        call_request_format: JSON Schema for the call request payload.
    """

    id: str = Field(description="Trigger type identifier.")
    description: str = Field(description="Human-readable description.")
    trigger_request_format: Any | None = Field(
        default=None, description="JSON Schema for trigger configuration."
    )
    call_request_format: Any | None = Field(
        default=None, description="JSON Schema for the call request payload."
    )


WorkerStatus = Literal["connected", "available", "busy", "disconnected"]


class WorkerInfo(BaseModel):
    """Information about a connected worker.

    Attributes:
        id: Engine-assigned unique worker ID.
        name: Worker name from InitOptions.
        runtime: SDK runtime (``python``, ``node``, ``rust``).
        version: SDK version string.
        os: Operating system identifier.
        ip_address: Worker's IP address as seen by the engine.
        status: Current status (``connected``, ``available``, ``busy``, ``disconnected``).
        connected_at_ms: Connection timestamp in milliseconds since epoch.
        function_count: Number of registered functions.
        functions: List of registered function IDs.
        active_invocations: Number of currently executing invocations.
    """

    id: str = Field(description="Engine-assigned unique worker ID.")
    name: str | None = Field(default=None, description="Worker name from InitOptions.")
    runtime: str | None = Field(
        default=None,
        description="SDK runtime (``python``, ``node``, ``rust``).",
    )
    version: str | None = Field(default=None, description="SDK version string.")
    os: str | None = Field(default=None, description="Operating system identifier.")
    ip_address: str | None = Field(
        default=None,
        description="Worker's IP address as seen by the engine.",
    )
    status: WorkerStatus = Field(
        description="Current status (``connected``, ``available``, ``busy``, ``disconnected``)."
    )
    connected_at_ms: int = Field(description="Connection timestamp in milliseconds since epoch.")
    function_count: int = Field(description="Number of registered functions.")
    functions: list[str] = Field(description="List of registered function IDs.")
    active_invocations: int = Field(description="Number of currently executing invocations.")


class StreamChannelRef(BaseModel):
    """Reference to a streaming channel for worker-to-worker data transfer.

    Attributes:
        channel_id: Unique channel identifier.
        access_key: Secret key for authenticating channel access.
        direction: Channel direction (``read`` or ``write``).
    """

    channel_id: str = Field(description="Unique channel identifier.")
    access_key: str = Field(description="Secret key for authenticating channel access.")
    direction: Literal["read", "write"] = Field(description="Channel direction (``reader`` or ``writer``).")


class OtelLogEvent(BaseModel):
    """OTEL log event received from the engine via ``on_log``."""

    timestamp_unix_nano: int
    observed_timestamp_unix_nano: int
    severity_number: int
    severity_text: str
    body: str
    attributes: dict[str, Any] = Field(default_factory=dict)
    trace_id: str | None = None
    span_id: str | None = None
    resource: dict[str, str] = Field(default_factory=dict)
    service_name: str = ""
    instrumentation_scope_name: str | None = None
    instrumentation_scope_version: str | None = None


IIIMessage = (
    RegisterFunctionMessage
    | UnregisterFunctionMessage
    | InvokeFunctionMessage
    | InvocationResultMessage
    | RegisterServiceMessage
    | RegisterTriggerMessage
    | RegisterTriggerTypeMessage
    | UnregisterTriggerMessage
    | UnregisterTriggerTypeMessage
    | TriggerRegistrationResultMessage
    | WorkerRegisteredMessage
)
