"""III message types."""

from enum import Enum
from typing import Any, Literal

from iii_helpers.http import HttpInvocationConfig
from pydantic import BaseModel, ConfigDict, Field


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
        id: Unique identifier for the trigger type (e.g. ``state``, ``durable:subscriber``).
        description: Human-readable description of what this trigger type does.
        trigger_request_format: JSON Schema describing the expected trigger config.
        call_request_format: JSON Schema describing the payload sent to functions.
    """

    id: str = Field(description="Unique identifier for the trigger type (e.g. ``state``, ``durable:subscriber``).")
    description: str = Field(description="Human-readable description of what this trigger type does.")
    trigger_request_format: Any | None = Field(
        default=None, description="JSON Schema for trigger configuration."
    )
    call_request_format: Any | None = Field(
        default=None, description="JSON Schema for the call request payload."
    )


class RegisterTriggerInput(BaseModel):
    """Input for registering a trigger (matches Node SDK's RegisterTriggerInput).

    Attributes:
        type: Identifier of the registered trigger type this trigger uses (e.g. ``storage::object-created``, ``http``).
        function_id: ID of the function this trigger invokes when it fires.
        config: Trigger-type-specific configuration, matching the shape the trigger type expects.
        metadata: Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation.
    """

    type: str = Field(
        description="Identifier of the registered trigger type this trigger uses (e.g. ``storage::object-created``, ``http``)."
    )
    function_id: str = Field(description="ID of the function this trigger invokes when it fires.")
    config: Any = Field(
        default=None, description="Trigger-type-specific configuration, matching the shape the trigger type expects."
    )
    metadata: Any | None = Field(
        default=None,
        description="Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation.",
    )


class RegisterTriggerMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    trigger_type: str = Field(alias="trigger_type")
    function_id: str = Field()
    config: Any
    metadata: Any | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.REGISTER_TRIGGER, alias="type")


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
    """Input for registering a function, matches Node.js RegisterFunctionInput.

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
    metadata: Any | None = Field(default=None, description="Arbitrary metadata attached to the function.")
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
    metadata: Any | None = None
    invocation: HttpInvocationConfig | None = None
    message_type: MessageType = Field(default=MessageType.REGISTER_FUNCTION, alias="type")


class TriggerActionEnqueue(BaseModel):
    """Routes the invocation through a named queue for async processing.

    Requires a queue worker in the project — run ``iii worker add queue``.
    Without it the trigger rejects with ``enqueue_error`` (no queue provider).

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


class MiddlewareFunctionInput(BaseModel):
    """Input passed to the RBAC middleware function on every function invocation
    through the RBAC port.

    Attributes:
        function_id: ID of the function being invoked.
        payload: Payload sent by the caller.
        action: Routing action, if any.
        context: Auth context returned by the auth function for this session.
    """

    function_id: str = Field(description="ID of the function being invoked.")
    payload: dict[str, Any] = Field(description="Payload sent by the caller.")
    action: TriggerActionEnqueue | TriggerActionVoid | None = Field(
        default=None, description="Routing action, if any.",
    )
    context: dict[str, Any] = Field(
        description="Auth context returned by the auth function for this session.",
    )


class TriggerRequest(BaseModel):
    """Request object for ``trigger()``.

    Attributes:
        function_id: ID of the function to invoke.
        payload: Input data passed to the function.
        action: Sets how the trigger is routed. Omit for a synchronous
            request/response. Specify for a specific routing scheme (e.g.
            ``TriggerAction.Enqueue(...)``, ``TriggerAction.Void()``).
        timeout_ms: Override the default invocation timeout, in milliseconds.
        metadata: Arbitrary user-specifiable metadata supplied to the triggered
            handler function on every invocation.
    """

    function_id: str = Field(description="ID of the function to invoke.")
    payload: Any = Field(default=None, description="Input data passed to the function.")
    action: TriggerActionEnqueue | TriggerActionVoid | None = Field(
        default=None,
        description=(
            "Sets how the trigger is routed. Omit for a synchronous request/response. "
            "Specify for a specific routing scheme (e.g. ``TriggerAction.Enqueue(...)``, "
            "``TriggerAction.Void()``)."
        ),
    )
    timeout_ms: int | None = Field(
        default=None, description="Override the default invocation timeout, in milliseconds."
    )
    metadata: Any | None = Field(
        default=None,
        description="Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation.",
    )


class InvokeFunctionMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    function_id: str = Field()
    data: Any
    metadata: Any | None = Field(default=None)
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
    | RegisterTriggerMessage
    | RegisterTriggerTypeMessage
    | UnregisterTriggerMessage
    | UnregisterTriggerTypeMessage
    | TriggerRegistrationResultMessage
    | WorkerRegisteredMessage
)
