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
#: The host delivers the calls of the functions it hosts, local and routed: ``observed`` on
#: ``invocationresult``, ``output_is_input`` on report items, and the top-level ``args_format``,
#: ``private`` and ``observable`` of ``registerfunction``.
FEATURE_OBSERVE_V2 = "observe.v2"
#: The engine confirms functions for in-process calls (``functionregistered.local_dispatch``).
FEATURE_LOCAL_DISPATCH_V1 = "local-dispatch.v1"
#: Declared by the SDK only: it receives JSON messages up to 64 MiB, so the engine sends it
#: messages of that size whatever version it reports.
FEATURE_LARGE_MESSAGES_V1 = "large-messages.v1"
#: What this SDK declares in the ``engine::workers::register`` payload.
SDK_FEATURES: tuple[str, ...] = (
    FEATURE_OBSERVE_V1,
    FEATURE_OBSERVE_V2,
    FEATURE_LOCAL_DISPATCH_V1,
    FEATURE_LARGE_MESSAGES_V1,
)

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
    and on a call through an observer (``call({"observer": obs, ...})``, worker
    → engine, with ``observer_id`` only)."""

    observer_id: str
    activation_id: str | None = None


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


class ObserverInfoWire(_WireModel):
    """An observer as the host of its function sees it (``observeradded.observer``,
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

    @field_validator("args", mode="before")
    @classmethod
    def _object(cls, v: Any) -> Any:
        return v if isinstance(v, dict) else {}

    @field_validator("capture", mode="before")
    @classmethod
    def _strings(cls, v: Any) -> Any:
        return [c for c in v if isinstance(c, str)] if isinstance(v, list) else list(DEFAULT_CAPTURE)


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
    """The host accepts (or, with ``error``, rejects) an added observer."""

    function_id: str
    namespace: str | None = None
    id: str
    revision: int
    error: ErrorBody | None = None
    message_type: MessageType = Field(default=MessageType.OBSERVER_ACK, alias="type")


class ObservedInvocationWire(_WireModel):
    """One ``invocationsobserved`` item: a local call of this worker's
    function and the observers iii's delivery matched (``matched``).

    ``input``, ``output``, ``error`` and ``metadata`` keep ``null`` apart from
    absent (``"output": null`` is a function that returned ``None``); check
    ``model_fields_set``. ``output_is_input`` (sent only when ``true``) means
    the handler returned its input: the engine fills ``output`` from ``input``.
    The reporter writes items itself.
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
    output_is_input: bool | None = None


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
    #: ``BUFFER_FULL``, ``TOO_LARGE``, ``DISCONNECTED`` or ``INACTIVE``.
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
    #: JSON Schema of the observers' ``args``; the engine validates them (``INVALID_ARGS``).
    args_format: Any | None = None
    #: ``True``: only this connection may call the function. Left out when ``False``.
    private: bool | None = None
    #: ``False``: nobody may observe the function (``NOT_OBSERVABLE``). Left out when ``True``.
    observable: bool | None = None
    message_type: MessageType = Field(default=MessageType.REGISTER_FUNCTION, alias="type")


class RegistrationRejectedMessage(_WireModel):
    """The engine refused a registration: one function (``function_id`` set) or
    the whole worker (``WORKER_NAMESPACE_CONFLICT``)."""

    code: str = ""
    namespace: str = ""
    worker_name: str | None = None
    function_id: str | None = None
    owner_worker_id: str = ""
    message_type: MessageType = Field(default=MessageType.REGISTRATION_REJECTED, alias="type")


class CallActionEnqueue(BaseModel):
    """Routes the call through a named queue for async processing.

    Requires the ``queue`` worker in ``worker-compose.yaml`` and a matching
    entry under that worker's ``queue_configs``.
    Without it the call rejects with ``enqueue_error`` (no queue provider).

    Attributes:
        type: Always ``'enqueue'``.
        queue: Name of the target queue.
    """

    type: Literal["enqueue"] = "enqueue"
    queue: str


class CallActionVoid(BaseModel):
    """Fire-and-forget routing. No response is returned.

    Attributes:
        type: Always ``'void'``.
    """

    type: Literal["void"] = "void"


CallAction = CallActionEnqueue | CallActionVoid
"""Routing action for call requests."""

TriggerActionEnqueue = CallActionEnqueue
"""Deprecated alias of :class:`CallActionEnqueue`."""

TriggerActionVoid = CallActionVoid
"""Deprecated alias of :class:`CallActionVoid`."""


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
    action: CallActionEnqueue | CallActionVoid | None = Field(
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


class CallRequest(BaseModel):
    """Request object for ``call()`` / ``call_async()``.

    Name the function to call with ``function_id``, or call through an
    observer with ``observer``: the call runs the observer's target, and only
    the worker that hosts the observed function may make it.

    Attributes:
        function_id: ID of the function to call.
        observer: An :class:`~iii.Observer` of a function this worker hosts:
            the call runs its target, with its policy and metadata sidecar.
            Takes no ``function_id``, ``namespace``, ``metadata`` or enqueue
            action.
        payload: Input data passed to the function.
        action: Sets how the call is routed. Omit for a synchronous
            request/response. Specify for a specific routing scheme (e.g.
            ``CallAction.Enqueue(...)``, ``CallAction.Void()``).
        timeout_ms: Override the default invocation timeout, in milliseconds.
        metadata: Arbitrary user-specifiable metadata supplied to the called
            handler function on every invocation.
        namespace: Target namespace for routing. Omit to inherit this worker's;
            say ``default`` to reach the engine's from a namespaced worker.
        dispatch: ``auto`` (default) runs a function this worker hosts
            in-process once the engine confirmed it; ``remote`` always goes
            through the engine.
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    function_id: str | None = Field(default=None, description="ID of the function to call.")
    observer: Any = Field(
        default=None,
        description=(
            "An Observer of a function this worker hosts: the call runs its target. "
            "Takes no function_id, namespace, metadata or enqueue action."
        ),
    )
    payload: Any = Field(default=None, description="Input data passed to the function.")
    action: CallActionEnqueue | CallActionVoid | None = Field(
        default=None,
        description=(
            "Sets how the call is routed. Omit for a synchronous request/response. "
            "Specify for a specific routing scheme (e.g. ``CallAction.Enqueue(...)``, "
            "``CallAction.Void()``)."
        ),
    )
    timeout_ms: int | None = Field(
        default=None, description="Override the default invocation timeout, in milliseconds."
    )
    metadata: Any | None = Field(
        default=None,
        description=(
            "Arbitrary user-specifiable metadata supplied to the called handler function on every invocation."
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

    @model_validator(mode="after")
    def _one_target(self) -> "CallRequest":
        if (self.function_id is None) == (self.observer is None):
            raise ValueError("a call names a function_id or an observer, not both")
        return self


TriggerRequest = CallRequest
"""Deprecated alias of :class:`CallRequest`."""


class InvokeFunctionMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    function_id: str = Field()
    data: Any
    metadata: Any | None = Field(default=None)
    invocation_id: str | None = Field(default=None)
    traceparent: str | None = Field(default=None)
    baggage: str | None = Field(default=None)
    action: CallActionEnqueue | CallActionVoid | None = Field(default=None)
    namespace: str | None = Field(default=None)
    #: The handler the call is made from (worker → engine), or who started it,
    #: stamped by the engine (engine → worker).
    caller: CallerWire | None = Field(default=None)
    #: Depth of the causal chain in observe hops; left out when 0.
    observe_depth: int | None = Field(default=None)
    #: Present on every activation (engine → worker) and on a call through an
    #: observer (worker → engine).
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
    #: The observers this host's delivery of a routed call matched, when the
    #: answer is the handler's own outcome; left out when empty.
    observed: list[str] | None = Field(default=None)
    message_type: MessageType = Field(default=MessageType.INVOCATION_RESULT, alias="type")


class WorkerRegisteredMessage(_WireModel):
    worker_id: str = Field(default="")
    reattach_token: str | None = Field(default=None)
    #: What the engine supports (``observe.v1``, ``observe.v2``, ``local-dispatch.v1``); absent on a
    #: pre-0.25 engine.
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
    | WorkerRegisteredMessage
    | RegistrationRejectedMessage
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
