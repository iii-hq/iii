"""III message types."""

from enum import Enum
from typing import Any, ClassVar, Literal

from iii_helpers.http import HttpInvocationConfig
from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator


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
    REGISTRATION_REJECTED = "registrationrejected"
    REATTACH = "reattach"
    # observe (0.25): sent only to and from connections that declared `observe.v1`
    OBSERVE = "observe"
    UNOBSERVE = "unobserve"
    OBSERVE_RESULT = "observeresult"
    OBSERVER_STATE_CHANGED = "observerstatechanged"
    FUNCTION_REGISTERED = "functionregistered"
    OBSERVER_ADDED = "observeradded"
    OBSERVER_REMOVED = "observerremoved"
    OBSERVER_ACK = "observerack"
    INVOCATIONS_OBSERVED = "invocationsobserved"
    LOCAL_INVOCATION_STATS = "localinvocationstats"


#: The engine supports observers (``WorkerRegistered.features``).
FEATURE_OBSERVE_V1 = "observe.v1"
#: The engine confirms functions for in-process calls (``functionregistered.local_dispatch``).
FEATURE_LOCAL_DISPATCH_V1 = "local-dispatch.v1"
#: What this SDK declares in the ``engine::workers::register`` payload.
SDK_FEATURES: tuple[str, ...] = (FEATURE_OBSERVE_V1, FEATURE_LOCAL_DISPATCH_V1)

#: What an absent ``capture`` means (CONTRACT D1).
DEFAULT_CAPTURE: tuple[str, ...] = ("input", "output", "caller")


class _WireModel(BaseModel):
    """A protocol message or part of one.

    Decoding is lenient, as the wire contract asks
    (``sdk/fixtures/protocol/observe/README.md``): ``null`` for an optional
    field reads as absent (its default), unknown fields are ignored, and enum
    values this SDK does not know are kept as strings. Encoding (``_to_dict``
    with ``exclude_none``) leaves unset optional fields out.
    """

    model_config = ConfigDict(populate_by_name=True)

    #: Fields whose ``null`` is a value, not "absent".
    _keep_null: ClassVar[frozenset[str]] = frozenset()

    @model_validator(mode="before")
    @classmethod
    def _null_is_absent(cls, data: Any) -> Any:
        if isinstance(data, dict):
            return {k: v for k, v in data.items() if v is not None or k in cls._keep_null}
        return data


class RegisterTriggerTypeMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    description: str
    trigger_request_format: Any | None = Field(default=None)
    call_request_format: Any | None = Field(default=None)
    #: Namespace this provider serves. ``None`` lets the engine use this
    #: connection's own, which is what a worker providing a trigger type for
    #: its own project wants.
    namespace: str | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.REGISTER_TRIGGER_TYPE, alias="type")


class UnregisterTriggerTypeMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    #: The namespace the trigger type was registered in; ``None`` is this connection's.
    namespace: str | None = Field(default=None)
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
        namespace: Namespace the target function resolves in.
        trigger_namespace: Namespace the trigger type's provider is found in.
    """

    type: str = Field(
        description=(
            "Identifier of the registered trigger type this trigger uses "
            "(e.g. ``storage::object-created``, ``http``)."
        )
    )
    function_id: str = Field(description="ID of the function this trigger invokes when it fires.")
    config: Any = Field(
        default=None, description="Trigger-type-specific configuration, matching the shape the trigger type expects."
    )
    metadata: Any | None = Field(
        default=None,
        description=(
            "Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation."
        ),
    )
    namespace: str | None = Field(
        default=None,
        description=(
            "Namespace the trigger's target function resolves in. Omitting it does not "
            "bind in the engine's default: ``register_trigger`` fills it from this "
            "worker's namespace, because the function a trigger names is one this worker "
            "registered, and that landed in the worker's namespace. Name another "
            "namespace, ``default`` included, to bind elsewhere."
        ),
    )
    trigger_namespace: str | None = Field(
        default=None,
        description=(
            "Namespace to find the trigger type's provider in. Omitting it is not the "
            "same as 'default': it asks the engine to resolve, taking this worker's "
            "namespace first and the engine's own second. Naming one is strict."
        ),
    )


class RegisterTriggerMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str
    trigger_type: str = Field(alias="trigger_type")
    function_id: str = Field()
    config: Any
    metadata: Any | None = Field(default=None)
    namespace: str | None = Field(default=None)
    #: Namespace to find the trigger type's provider in. ``None`` is not
    #: ``"default"``: it asks the engine to resolve, taking this worker's
    #: namespace first and the engine's own second. Naming one is strict.
    trigger_namespace: str | None = Field(default=None)
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


class ErrorBody(_WireModel):
    """``{code, message, stacktrace?}``: an error or a state reason."""

    code: str = ""
    message: str = ""
    stacktrace: str | None = None

    @field_validator("code", "message", mode="before")
    @classmethod
    def _text(cls, v: Any) -> str:
        return v if isinstance(v, str) else str(v)


class CallerParent(_WireModel):
    """``caller.parent``: one level only (a ``parent`` inside it is dropped)."""

    worker_id: str | None = None
    worker_name: str | None = None
    function_id: str | None = None
    invocation_id: str | None = None

    @model_validator(mode="before")
    @classmethod
    def _strings_only(cls, data: Any) -> Any:
        if isinstance(data, dict):
            keys = ("worker_id", "worker_name", "function_id", "invocation_id", "parent")
            return {k: v for k, v in data.items() if k in keys and (k == "parent" or isinstance(v, str))}
        return data


class CallerWire(CallerParent):
    """Who started an invocation (protocol.md §2.4).

    ``worker_id`` / ``worker_name`` are stamped by the engine from the calling
    connection; ``function_id`` / ``invocation_id`` / ``parent`` are asserted
    by the calling SDK.
    """

    parent: CallerParent | None = None

    @field_validator("parent", mode="before")
    @classmethod
    def _parent_object(cls, v: Any) -> Any:
        return v if isinstance(v, dict) else None


class ActivationWire(_WireModel):
    """``invokefunction.activation``: present on every activation (engine → worker)
    and on an owner delivery (``obs.call``, worker → engine)."""

    observer_id: str
    activation_id: str | None = None


class ObserveDescriptor(_WireModel):
    """``registerfunction.observe``: the four fields that travel (the hooks never do)."""

    delivery: str | None = None
    args_format: Any | None = None
    legacy_trigger_type: str | None = None
    allow: bool | None = None


class ObserveSourceWire(_WireModel):
    """``observe.source``: the left side of ``iii.observe``."""

    function_id: str
    namespace: str | None = None
    args: dict[str, Any] | None = None
    filters: dict[str, Any] | None = None
    capture: list[str] | None = None
    on: str | None = None
    deliver: str | None = None


class FunctionTargetWire(_WireModel):
    """``observe.target``: the right side of ``iii.observe``, a registered function."""

    function_id: str
    namespace: str | None = None


class ObserveMessage(_WireModel):
    """Create, or idempotently replay, an observer owned by this connection."""

    id: str
    source: ObserveSourceWire
    target: FunctionTargetWire
    metadata: Any | None = None
    max_fires: int | None = None
    expires_in_ms: int | None = None
    max_in_flight: int | None = None
    message_type: MessageType = Field(default=MessageType.OBSERVE, alias="type")


class UnobserveMessage(_WireModel):
    id: str
    message_type: MessageType = Field(default=MessageType.UNOBSERVE, alias="type")


class ObserveResultMessage(_WireModel):
    """The reply to ``observe`` (also to a replay): the observer's current state."""

    id: str
    state: str
    error: ErrorBody | None = None
    warnings: list[str] | None = None
    message_type: MessageType = Field(default=MessageType.OBSERVE_RESULT, alias="type")


class ObserverStateChangedMessage(_WireModel):
    """A lifecycle transition, sent to the observing worker."""

    id: str
    state: str
    reason: ErrorBody | None = None
    fires: int = 0
    message_type: MessageType = Field(default=MessageType.OBSERVER_STATE_CHANGED, alias="type")


class LegacyTriggerWire(_WireModel):
    """``observer.legacy``: the old trigger an observer of the legacy shim came from."""

    _keep_null: ClassVar[frozenset[str]] = frozenset({"config"})

    trigger_type: str
    config: Any = None


class ObserverInfoWire(_WireModel):
    """An observer as its owner sees it (``observeradded.observer``,
    ``functionregistered.observers[]``): never its metadata, policy or worker."""

    id: str = ""
    args: dict[str, Any] = Field(default_factory=dict)
    #: Normal form (filters.md §5). Absent or ``null`` is ``{}``; anything else
    #: is kept as sent, so a malformed one is refused (``INVALID_FILTER``)
    #: instead of read as "match everything".
    filters: Any = Field(default_factory=dict)
    target: str = ""
    capture: list[str] = Field(default_factory=lambda: list(DEFAULT_CAPTURE))
    on: str = "success"
    legacy: LegacyTriggerWire | None = None

    @field_validator("args", mode="before")
    @classmethod
    def _object(cls, v: Any) -> Any:
        return v if isinstance(v, dict) else {}

    @field_validator("capture", mode="before")
    @classmethod
    def _strings(cls, v: Any) -> Any:
        return [c for c in v if isinstance(c, str)] if isinstance(v, list) else list(DEFAULT_CAPTURE)

    @field_validator("legacy", mode="before")
    @classmethod
    def _legacy(cls, v: Any) -> Any:
        return v if isinstance(v, dict) and isinstance(v.get("trigger_type"), str) else None


class FunctionRegisteredMessage(_WireModel):
    """The ``registerfunction`` ack, with the full current observer set."""

    id: str = ""
    namespace: str = ""
    revision: int = 0
    local_dispatch: bool = False
    observers: list[ObserverInfoWire] = Field(default_factory=list)
    message_type: MessageType = Field(default=MessageType.FUNCTION_REGISTERED, alias="type")


class ObserverAddedMessage(_WireModel):
    """One observer attached to a function this connection hosts; awaits ``observerack``."""

    function_id: str = ""
    namespace: str = ""
    revision: int = 0
    observer: ObserverInfoWire = Field(default_factory=ObserverInfoWire)
    message_type: MessageType = Field(default=MessageType.OBSERVER_ADDED, alias="type")


class ObserverRemovedMessage(_WireModel):
    function_id: str = ""
    namespace: str = ""
    revision: int = 0
    id: str = ""
    message_type: MessageType = Field(default=MessageType.OBSERVER_REMOVED, alias="type")


class ObserverAckMessage(_WireModel):
    """The owner accepts (or, with ``error``, rejects) an added observer."""

    function_id: str
    namespace: str | None = None
    id: str
    revision: int
    error: ErrorBody | None = None
    message_type: MessageType = Field(default=MessageType.OBSERVER_ACK, alias="type")


class ObservedInvocationWire(_WireModel):
    """One ``invocationsobserved`` item: a matched local call of an auto
    function, or an ``obs.emit`` event of an owner function.

    ``input``, ``output``, ``error`` and ``metadata`` keep ``null`` apart from
    absent (``"output": null`` is a function that returned ``None``); check
    ``model_fields_set``. The reporter writes items itself.
    """

    _keep_null: ClassVar[frozenset[str]] = frozenset({"input", "output", "error", "metadata"})

    function_id: str
    namespace: str | None = None
    invocation_id: str
    matched: list[str] = Field(default_factory=list)
    status: str = "success"
    input: Any = None
    output: Any = None
    error: Any = None
    caller: CallerWire | None = None
    metadata: Any = None
    started_at_ms: int = 0
    duration_ms: float = 0.0
    observe_depth: int = 0
    traceparent: str | None = None
    baggage: str | None = None


class InvocationsObservedMessage(_WireModel):
    items: list[ObservedInvocationWire] = Field(default_factory=list)
    message_type: MessageType = Field(default=MessageType.INVOCATIONS_OBSERVED, alias="type")


class LocalInvocationCount(_WireModel):
    function_id: str
    count: int = 0
    errors: int = 0
    duration_ms_sum: float = 0.0


class DroppedReportCount(_WireModel):
    function_id: str
    #: ``BUFFER_FULL`` or ``TOO_LARGE`` (protocol.md §4).
    reason: str
    count: int = 0


class LocalInvocationStatsMessage(_WireModel):
    """Periodic counts of local invocations and SDK-side report drops (every 10 s, when non-zero)."""

    window_ms: int = 0
    functions: list[LocalInvocationCount] = Field(default_factory=list)
    dropped_reports: list[DroppedReportCount] = Field(default_factory=list)
    message_type: MessageType = Field(default=MessageType.LOCAL_INVOCATION_STATS, alias="type")


class RegisterFunctionMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str = Field()
    description: str | None = None
    request_format: RegisterFunctionFormat | dict[str, Any] | None = Field(default=None)
    response_format: RegisterFunctionFormat | dict[str, Any] | None = Field(default=None)
    metadata: Any | None = None
    invocation: HttpInvocationConfig | None = None
    #: How the function's observers are delivered (``registerfunction.observe``).
    observe: ObserveDescriptor | None = None
    message_type: MessageType = Field(default=MessageType.REGISTER_FUNCTION, alias="type")


class TriggerActionEnqueue(BaseModel):
    """Routes the invocation through a named queue for async processing.

    Requires the ``queue`` worker in ``worker-compose.yaml`` and a matching
    entry under that worker's ``queue_configs``.
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
        namespace: Target namespace the invoke addressed; forward the call here
            to stay in the caller's namespace. Absent -> the engine's default
            namespace.
    """

    function_id: str = Field(description="ID of the function being invoked.")
    payload: dict[str, Any] = Field(description="Payload sent by the caller.")
    action: TriggerActionEnqueue | TriggerActionVoid | None = Field(
        default=None, description="Routing action, if any.",
    )
    context: dict[str, Any] = Field(
        description="Auth context returned by the auth function for this session.",
    )
    namespace: str | None = Field(
        default=None,
        description=(
            "Target namespace the invoke addressed; forward the call here to "
            "stay in the caller's namespace. Absent -> the engine's default "
            "namespace."
        ),
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
        namespace: Target namespace for routing. Omit to inherit this worker's;
            say ``default`` to reach the engine's from a namespaced worker.
        dispatch: ``auto`` (default) runs a function this worker hosts
            in-process once the engine confirmed it; ``remote`` always goes
            through the engine.
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
        description=(
            "Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation."
        ),
    )
    namespace: str | None = Field(
        default=None,
        description=(
            "Target namespace for routing. Omit to inherit this worker's; say ``default`` "
            "to reach the engine's from a namespaced worker."
        ),
    )
    dispatch: Literal["auto", "remote"] | None = Field(
        default=None,
        description=(
            "``auto`` (default): a function this worker hosts, once the engine confirmed it, runs "
            "in-process. ``remote``: always through the engine (for callers that rely on "
            "engine-side effects)."
        ),
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
    namespace: str | None = Field(default=None)
    #: The handler the call is made from (worker → engine), or who started it,
    #: stamped by the engine (engine → worker).
    caller: CallerWire | None = Field(default=None)
    #: Depth of the causal chain in observe hops; left out when 0.
    observe_depth: int | None = Field(default=None)
    #: Present on every activation; marks an owner delivery (``obs.call``) worker → engine.
    activation: ActivationWire | None = Field(default=None)
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


class WorkerRegisteredMessage(_WireModel):
    worker_id: str = Field(default="")
    reattach_token: str | None = Field(default=None)
    #: What the engine supports (``observe.v1``, ``local-dispatch.v1``); absent on a pre-0.25 engine.
    features: list[str] | None = Field(default=None)
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
    | ObserveMessage
    | UnobserveMessage
    | ObserveResultMessage
    | ObserverStateChangedMessage
    | FunctionRegisteredMessage
    | ObserverAddedMessage
    | ObserverRemovedMessage
    | ObserverAckMessage
    | InvocationsObservedMessage
    | LocalInvocationStatsMessage
)
