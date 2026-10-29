"""III SDK implementation for WebSocket communication with the III Engine."""

import asyncio
import concurrent.futures
import contextvars
import copy
import inspect
import json
import logging
import os
import platform
import random
import threading
import time
import traceback
import uuid
from collections.abc import Iterator
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from dataclasses import dataclass, field, replace
from importlib.metadata import version
from typing import Any, Awaitable, Callable, Coroutine, TypeVar, cast, overload

import websockets
from iii_helpers.http import HttpInvocationConfig
from iii_helpers.observability import OtelConfig
from iii_helpers.stream import (
    StreamDeleteInput,
    StreamGetInput,
    StreamListGroupsInput,
    StreamListInput,
    StreamSetInput,
)
from websockets.asyncio.client import ClientConnection

from . import _wire_json
from .channels import ChannelReader, ChannelWriter, resolve_headers
from .errors import InvocationError, ObserveRejection, RegistrationRejectedError, _wrap_wire_error
from .filter._value import ABSENT, to_json_value
from .format_utils import extract_request_format, extract_response_format
from .function_ref import FunctionRef
from .iii_constants import (
    DEFAULT_ENGINE_URL,
    DEFAULT_RECONNECTION_CONFIG,
    MAX_QUEUE_SIZE,
    ConnectionStateCallback,
    IIIConnectionState,
    InitOptions,
)
from .iii_types import (
    FEATURE_OBSERVE_V1,
    FEATURE_OBSERVE_V2,
    SDK_FEATURES,
    ActivationWire,
    CallActionEnqueue,
    CallActionVoid,
    CallerWire,
    CallRequest,
    FunctionRegisteredMessage,
    InvocationResultMessage,
    InvokeFunctionMessage,
    MessageType,
    ObserverAddedMessage,
    ObserveResultMessage,
    ObserverRemovedMessage,
    ObserverStateChangedMessage,
    RegisterFunctionFormat,
    RegisterFunctionInput,
    RegisterFunctionMessage,
    RegistrationRejectedMessage,
    StreamChannelRef,
    UnobserveMessage,
    UnregisterFunctionMessage,
    WorkerRegisteredMessage,
)
from .invocation_context import (
    ActiveInvocation,
    InvocationContext,
    current_context,
    guard_exit,
    original_error,
    public_context,
    reset_context,
    run_in_thread,
    set_context,
)
from .observable import (
    OBSERVE_HOOK_TIMEOUT_S,
    ActiveSet,
    CallLike,
    Delivery,
    FunctionObservers,
    HookQueue,
    Level,
    Observer,
    is_async_callable,
)
from .observe_types import (
    AddedHook,
    Call,
    DeliverHook,
    Key,
    MatchHook,
    MatchKeys,
    MatchPredicate,
    ObserveReason,
    ObserverOptions,
    ObserveSource,
    RemovedHook,
    Target,
)
from .observe_wire import (
    build_observe_message,
    build_observer_ack,
    caller_for,
    decode_activation,
    decode_caller,
    decode_error,
    decode_observe_depth,
    snapshot,
    strip_caller,
)
from .observer_handle import ObserverHandle, to_observer_state
from .process_marker import announce_process
from .reporter import FunctionStatus, Reporter
from .stream import IStream
from .types import Channel, RemoteFunctionData, is_channel_ref

RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]
TResult = TypeVar("TResult")
AddedT = TypeVar("AddedT", bound=AddedHook)
RemovedT = TypeVar("RemovedT", bound=RemovedHook)
MatchT = TypeVar("MatchT", bound=MatchPredicate)
DeliverT = TypeVar("DeliverT", bound=DeliverHook)

log = logging.getLogger("iii.iii")

# JSON envelopes up to this size are sent as one text frame.
_MAX_JSON_FRAME_BYTES = 16 * 1024 * 1024
# Outbound and inbound JSON message limit (67,108,864 bytes).
_MAX_JSON_OUTBOUND_BYTES = 64 * 1024 * 1024
_MAX_JSON_MESSAGE_BYTES = 64 * 1024 * 1024
# Python websockets takes strings per fragment, so boundaries are UTF-8 safe.
_JSON_FRAGMENT_BYTES = 1024 * 1024


def _json_fragments(payload: bytes) -> Iterator[str]:
    """Yield UTF-8 text fragments no larger than the frame payload limit."""
    offset = 0
    while offset < len(payload):
        end = min(offset + _JSON_FRAGMENT_BYTES, len(payload))
        while end < len(payload) and payload[end] & 0b11000000 == 0b10000000:
            end -= 1
        yield payload[offset:end].decode("utf-8")
        offset = end


#: Frames of the observe family: held while a new connection has not said
#: whether its engine supports observe, and dropped when it does not.
_OBSERVE_FAMILY = frozenset(
    {
        MessageType.OBSERVE.value,
        MessageType.UNOBSERVE.value,
        MessageType.OBSERVER_ACK.value,
        MessageType.INVOCATIONS_OBSERVED.value,
        MessageType.LOCAL_INVOCATION_STATS.value,
    }
)

_UNSUPPORTED_REASON: ObserveReason = {
    "code": "UNSUPPORTED",
    "message": "the engine does not support observe (no observe.v1 in its features); upgrade it to 0.25 or later",
}

#: Rejections of one function's registration: the connection stays open.
_FUNCTION_REJECTIONS = frozenset({"FUNCTION_NAMESPACE_CONFLICT", "INVALID_ARGS_FORMAT", "UNSUPPORTED"})

#: The second argument of a hook method that was not passed (``on_observer_added("id")``).
_MISSING: Any = object()


def _clone_value(value: Any) -> Any:
    """Local dispatch hands the handler a copy of the caller's payload and the
    caller a copy of the result (value semantics, as over the wire, README
    §11.8): a deep copy, or the JSON form for what cannot be copied."""
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    try:
        return copy.deepcopy(value)
    except Exception:
        # What the remote path would carry: members JSON cannot encode are left out.
        mapped = to_json_value(value)
        return None if mapped is ABSENT else mapped


def _handler_error_body(error: BaseException) -> dict[str, Any]:
    """The error a handler's failure becomes, the same on the local and the remote path."""
    return {
        "code": "invocation_failed",
        "message": str(error),
        "stacktrace": "".join(traceback.format_exception(type(error), error, error.__traceback__)),
    }


def _answer_error_body(error: BaseException) -> dict[str, Any]:
    """The error a call answers with when its ``on_observer_deliver`` raised:
    an :class:`ObserveRejection` or :class:`InvocationError` keeps its code,
    anything else is ``invocation_failed``."""
    if isinstance(error, ObserveRejection):
        return {"code": error.code, "message": error.message}
    if isinstance(error, InvocationError):
        body: dict[str, Any] = {"code": error.code, "message": error.message}
        if error.stacktrace is not None:
            body["stacktrace"] = error.stacktrace
        return body
    return _handler_error_body(error)


@dataclass
class _SpanOutcome:
    result: Any = None
    traceparent: str | None = None


#: sdk.md §3: a local call nested deeper than this goes through the engine.
_MAX_LOCAL_DEPTH = 64


@dataclass
class _Request:
    """One ``call`` request, normalized, with the path it takes."""

    function_id: str
    payload: Any
    action: Any
    metadata: Any
    namespace: str | None
    timeout_ms: int
    #: A call through this observer: it runs the observer's target.
    observer_id: str | None = None
    #: The local function when the call runs in-process.
    local: RemoteFunctionData | None = None
    state: FunctionObservers | None = None


@dataclass
class _LocalInvocation:
    """What a local call works on: its context, the input and metadata its
    handler sees, those its delivery sees (``call_input``,
    ``call_metadata``), and the active set and deliver hook it started
    with."""

    ctx: ActiveInvocation
    state: FunctionObservers
    active: ActiveSet
    hook: DeliverHook | None
    input: Any = None
    metadata: Any = None
    call_input: Any = None
    call_metadata: Any = None
    started_at_ms: float = field(default_factory=lambda: time.time() * 1000.0)
    t0: float = field(default_factory=time.perf_counter)

    @property
    def delivers(self) -> bool:
        """It has a deliver hook or observers: the call is delivered when it ends."""
        return self.hook is not None or bool(self.active.observers)


def _metadata_passing_mode(handler: Callable[..., Any]) -> str:
    """Decide how (if at all) to forward per-invocation metadata to a handler.

    Only handlers that explicitly declare a parameter named ``metadata``
    receive the sidecar; every other signature keeps its exact pre-metadata
    call shape (``handler(data)``), so existing handlers, including ones
    with unrelated optional parameters, ``*args``, or ``**kwargs``, keep
    working unchanged. Returns one of:

    - ``"positional"``: ``metadata`` is the second positional parameter
      (e.g. ``def handler(data, metadata=None)``).
    - ``"keyword"``: ``metadata`` is keyword-only
      (e.g. ``def handler(data, *, metadata=None)``).
    - ``"none"``: no ``metadata`` parameter; omit metadata.

    If the signature cannot be introspected (some builtins/C callables),
    falls back to ``"none"`` to preserve back-compat.
    """
    try:
        sig = inspect.signature(handler)
    except (ValueError, TypeError):
        return "none"

    meta_param = sig.parameters.get("metadata")
    if meta_param is None:
        return "none"
    if meta_param.kind == inspect.Parameter.KEYWORD_ONLY:
        return "keyword"

    positional = [
        p
        for p in sig.parameters.values()
        if p.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
    ]
    if len(positional) >= 2 and positional[1].name == "metadata":
        return "positional"
    return "none"


def _resolve_format(fmt: Any) -> Any | None:
    """Resolve a format value: if it's a type (e.g. Pydantic model), convert to JSON Schema."""
    if fmt is None:
        return None
    if isinstance(fmt, type):
        from .format_utils import python_type_to_format

        return python_type_to_format(fmt)
    return fmt


def _detect_project_name(cwd: str | None = None) -> str | None:
    """Return a project identifier for telemetry, derived from the current working directory.

    Reads ``[project] name`` from ``pyproject.toml`` if present at ``cwd``;
    otherwise falls back to the basename of ``cwd``. Returns ``None`` only
    when both signals are unavailable (e.g. cwd is the filesystem root, or
    the Python runtime has no TOML parser and no readable cwd basename).

    No directory walking, only inspects ``cwd`` itself, so the SDK never
    reads files outside the user's explicit working directory.
    """
    try:
        cwd = cwd or os.getcwd()
        manifest = os.path.join(cwd, "pyproject.toml")
        if os.path.isfile(manifest):
            import importlib

            try:
                tomllib = importlib.import_module("tomllib")  # Python 3.11+
            except ImportError:
                tomllib = None
            if tomllib is not None:
                with open(manifest, "rb") as fh:
                    data = tomllib.load(fh)
                name = data.get("project", {}).get("name")
                if isinstance(name, str) and name.strip():
                    return name.strip()
    except Exception:
        pass

    if not cwd:
        return None
    base = os.path.basename(cwd).strip()
    return base or None


class _TraceContextError(Exception):
    """Wraps a handler exception with the response traceparent from the active span."""

    def __init__(self, traceparent: str | None) -> None:
        self.traceparent = traceparent


@dataclass(frozen=True)
class _PendingInvocation:
    """Pending invocation record kept on the SDK until the engine responds.

    ``function_id`` is preserved so the timeout and error-wrapping paths
    can name the target without plumbing it through every call site.
    """

    future: asyncio.Future[Any]
    function_id: str


class _ObserveHost:
    """What the host side (:class:`FunctionObservers`) and the reporter need
    from the client. Frames are only sent from the SDK loop."""

    reporter: Reporter

    def __init__(self, client: "III") -> None:
        self._client = client

    # ── ReporterHost ───────────────────────────────────────────────────────

    def online(self) -> bool:
        return self._client._observe_online()

    def status(self, function_id: str) -> FunctionStatus:
        client = self._client
        fn = client._functions.get(function_id)
        state = fn.observe if fn is not None else None
        if not isinstance(state, FunctionObservers) or state.released_epoch == client._connection_epoch:
            return "gone"
        return "ready" if state.confirmed_epoch == client._connection_epoch else "wait"

    def send_frame(self, data: str, on_failure: Callable[[BaseException], None] | None = None) -> bool:
        return self._client._send_frame(data, on_failure)

    def unsent_bytes(self) -> int:
        return self._client._unsent_bytes()

    def call_soon(self, callback: Callable[[], None]) -> None:
        try:
            self._client._loop.call_soon_threadsafe(callback)
        except RuntimeError:
            pass  # the loop is closed (shutdown)

    def call_later(self, delay_s: float, callback: Callable[[], None]) -> None:
        try:
            self._client._loop.call_soon_threadsafe(self._client._loop.call_later, delay_s, callback)
        except RuntimeError:
            pass

    # ── ObserveHost ────────────────────────────────────────────────────────

    def identity(self) -> dict[str, str]:
        client = self._client
        identity = {"worker_name": client._worker_name()}
        if client._worker_id is not None:
            identity = {"worker_id": client._worker_id, **identity}
        return identity

    def send_ack(
        self,
        function_id: str,
        namespace: str | None,
        observer_id: str,
        revision: int,
        error: dict[str, str] | None,
        on_sent: Callable[[], None] | None = None,
    ) -> None:
        # Sent in order with the report frames (FIFO on the SDK loop); never
        # queued while disconnected: the next functionregistered re-acks.
        client = self._client
        data = client._prepare_json(build_observer_ack(function_id, namespace, observer_id, revision, error))

        def send() -> None:
            client._send_frame(data, on_sent=on_sent)

        if threading.current_thread() is client._thread:
            send()
        else:
            self.call_soon(send)

    def added_hook(self, function_id: str, level: Level) -> AddedHook | None:
        client = self._client
        return client._worker_added if level == "worker" else client._added_hooks.get(function_id)

    def removed_hook(self, function_id: str, level: Level) -> RemovedHook | None:
        client = self._client
        return client._worker_removed if level == "worker" else client._removed_hooks.get(function_id)

    def match_hook(self, function_id: str) -> MatchHook | None:
        return self._client._match_hooks.get(function_id)

    def hook_queue(self, function_id: str) -> HookQueue:
        queues = self._client._hook_queues
        queue = queues.get(function_id)
        if queue is None:
            queue = queues[function_id] = HookQueue(function_id)
        return queue

    async def invoke_hook(self, hook: AddedHook | RemovedHook, obs: Observer) -> None:
        if is_async_callable(hook):
            await cast(Awaitable[None], hook(obs))
            return
        # A sync hook runs on a thread of its own, so it may call the sync SDK API.
        result = await run_in_thread(hook, obs)
        if inspect.isawaitable(result):
            await result

    def hook_timeout_s(self) -> float:
        return self._client._observe_hook_timeout_s

    def trace_headers(self) -> tuple[str | None, str | None]:
        return self._client._inject_traceparent(), self._client._inject_baggage()

    def on_sdk_thread(self) -> bool:
        return threading.current_thread() is self._client._thread

    def delivers_v2(self) -> bool:
        return self._client._delivers_v2()


class III:
    """WebSocket client for communication with the III Engine.

    Use ``register_worker(address, options)`` as the primary entry point.
    It creates the client and blocks until the connection is established.

    Args:
        address: WebSocket URL of the III engine (e.g. ``ws://localhost:49134``).
            The constructor requires this value. ``register_worker`` can resolve it from
            ``III_URL`` or ``DEFAULT_ENGINE_URL``.
        options: Optional configuration. See ``InitOptions``.

    Examples:
        >>> from iii import register_worker, InitOptions
        >>> worker = register_worker('ws://localhost:49134', InitOptions(worker_name='my-worker'))
    """

    def _call_namespace(self, explicit: str | None, source: str) -> str | None:
        """The namespace one call or one binding resolves in.

        ``None`` inherits the worker's. A blank one raises: named and left empty
        asks for the opposite of what absent asks for, and the two are only ever
        confused by accident. Python's ``or`` treats ``""`` as falsy, so a blank
        namespace was silently replaced by the worker's here while Node and Rust
        forwarded it and Go dropped it -- one mistake, four behaviours.

        Raises:
            ValueError: when ``explicit`` is present and holds only whitespace.
        """
        if explicit is not None and not explicit.strip():
            raise ValueError(
                f"namespace is empty: {source} was set to {explicit!r}. Give it a name, or "
                "leave it unset to stay in this worker's namespace."
            )
        return explicit if explicit is not None else self._worker_namespace()

    def _invocation_namespace(
        self, explicit: str | None, function_id: str, source: str = "CallRequest.namespace"
    ) -> str | None:
        """Resolve one invocation namespace.

        An explicit namespace wins. Otherwise, engine-owned builtins
        (``engine::*`` and ``configuration::*``) resolve in ``default`` and all
        other calls inherit this worker's namespace.
        """
        namespace = self._call_namespace(explicit, source)
        if explicit is None and function_id.startswith(("engine::", "configuration::")):
            return "default"
        return namespace

    def _worker_namespace(self) -> str | None:
        """Effective worker namespace: explicit option, then ``III_NAMESPACE`` env,
        else ``None`` (the engine applies its ``default`` namespace).

        A declared-but-blank namespace raises rather than reading as "no
        namespace". The two mean opposite things: absent asks for the engine's
        default, blank names a namespace and gives nothing to name it with.
        Read as absent, the worker registers in ``default``, and every call and
        observer it makes now follows it there -- a whole project quietly
        serving from the wrong namespace, and the one thing an operator cannot
        see by reading the declaration.

        Raises:
            ValueError: when either source is present and holds only whitespace.
        """
        declared = self._options.namespace
        if declared is not None:
            if not declared.strip():
                raise ValueError(
                    f"namespace is empty: InitOptions.namespace was set to {declared!r}. "
                    "Give it a name, or leave it unset to register in `default`."
                )
            return declared

        # III_NAMESPACE is left alone when blank. ``FOO=`` is how a shell says
        # "not set" -- ``III_NAMESPACE=${NS}`` with NS unset produces exactly
        # that -- so reading it as absent is what the caller meant, and absent
        # is a namespace a worker may legitimately have none of. Only the
        # option is a mistake: nobody writes a namespace parameter and passes
        # nothing on purpose.
        managed = os.environ.get("III_NAMESPACE")
        return managed if managed and managed.strip() else None

    def __init__(self, address: str, options: InitOptions | None = None) -> None:
        self._address = address
        self._options = options or InitOptions()
        self._ws: ClientConnection | None = None
        self._functions: dict[str, RemoteFunctionData] = {}
        self._pending: dict[str, _PendingInvocation] = {}
        self._queue: list[dict[str, Any]] = []
        self._reconnect_task: asyncio.Task[None] | None = None
        self._running = False
        self._receiver_task: asyncio.Task[None] | None = None
        self._reconnection_config = (
            self._options.reconnection_config or DEFAULT_RECONNECTION_CONFIG
        )
        self._reconnect_attempt = 0
        self._connection_state: IIIConnectionState = "disconnected"
        # Must exist before the auto-connect below: the loop thread calls
        # _set_connection_state as soon as connect_async starts.
        self._connection_listeners: list[ConnectionStateCallback] = []
        self._worker_id: str | None = None
        # Set when the engine fatally rejects this worker's registration
        # (e.g. a namespace/name collision). Terminal: no reconnect follows.
        self._fatal_error: RegistrationRejectedError | None = None
        self._reattach_token: str | None = None

        # observe (0.25). Observers this worker created, replayed (same id) on
        # every connection, until disposed.
        self._observers: dict[str, ObserverHandle] = {}
        # The engine's features (`WorkerRegistered.features`), kept across a
        # disconnect until the next `WorkerRegistered` replaces them.
        self._engine_features: frozenset[str] = frozenset()
        # `WorkerRegistered` arrived on the current connection.
        self._registered_on_connection = False
        # Bumped on every connection: `functionregistered` confirms a function on one.
        self._connection_epoch = 0
        # Observe-family frames queued while disconnected, held until the new
        # connection's `WorkerRegistered` says whether the engine supports them.
        self._held_observe_frames: list[dict[str, Any]] = []
        self._hooks_inactive_warned: set[str] = set()
        self._observe_hook_timeout_s = OBSERVE_HOOK_TIMEOUT_S
        # Observer hooks: the worker's, and each function's by id. Kept across
        # unregister() and a later registration of the same id.
        self._worker_added: AddedHook | None = None
        self._worker_removed: RemovedHook | None = None
        self._added_hooks: dict[str, AddedHook] = {}
        self._removed_hooks: dict[str, RemovedHook] = {}
        self._match_hooks: dict[str, MatchHook] = {}
        self._deliver_hooks: dict[str, DeliverHook] = {}
        # The hooks of each function id run one at a time, across
        # unregister() and a later registration of the id (SDK loop).
        self._hook_queues: dict[str, HookQueue] = {}
        # Logged once each: a hook id no function of this worker has, and a
        # function an engine without observe.v2 cannot host.
        self._hook_ids_warned: set[str] = set()
        # The check for those ids runs this long after the connection is
        # registered, or after a hook is set by id, so the registrations of
        # the worker's setup code are in place by then.
        self._hook_id_check_delay_s = 1.0
        self._hook_id_check: asyncio.TimerHandle | None = None
        self._unsupported_warned: set[str] = set()
        # standby: functions waiting for their id to be free, and the retry period.
        self._standing_by: set[str] = set()
        self._standby_retry_s = 1.0
        # Tasks nobody awaits (local `void` calls, report frames): kept alive here.
        self._background: set[asyncio.Future[Any]] = set()
        # Bytes of frames handed to the socket and not written yet, and their writes.
        self._inflight_bytes = 0
        self._frame_sends: set[asyncio.Future[Any]] = set()
        # User callbacks that must not run on the event loop (observer state
        # listeners): one thread, so they run in order.
        self._callbacks = ThreadPoolExecutor(max_workers=1, thread_name_prefix="iii-callbacks")
        self._observe_host = _ObserveHost(self)
        self._reporter = Reporter(self._observe_host)
        self._observe_host.reporter = self._reporter

        # Background event loop thread
        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._loop.run_forever, daemon=False)
        self._thread.start()
        self._loop.call_soon_threadsafe(self._reporter.start)

        # Lets `iii ps` find this worker however it was started.
        self._remove_process_marker = announce_process(
            address,
            os.environ.get("III_WORKER_NAME") or self._options.worker_name or f"{platform.node()}:{os.getpid()}",
        )

        # Auto-connect (non-blocking, matches Node.js constructor behavior)
        self._connected_event = threading.Event()
        self._schedule_on_loop(self.connect_async())

    def _run_on_loop(self, coro: Coroutine[Any, Any, TResult]) -> TResult:
        if threading.current_thread() is self._thread:
            raise RuntimeError(
                "Cannot call sync SDK methods from the event loop thread. Use async handler methods instead."
            )
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result()

    def _schedule_on_loop(self, coro: Coroutine[Any, Any, object]) -> None:
        asyncio.run_coroutine_threadsafe(coro, self._loop)

    # Connection management

    def _wait_until_connected(self) -> None:
        """Block until the WebSocket connection to the engine is established."""
        # A terminal registration rejection (e.g. a worker-name collision) is not
        # retryable: surface the typed error so callers observe the export
        # contract instead of a generic ConnectionError or a hung call.
        if self._fatal_error is not None:
            raise self._fatal_error
        if self._connection_state == "connected":
            return
        if self._connection_state == "failed":
            raise ConnectionError(f"Connection to {self._address} failed")
        connected = self._connected_event.wait(timeout=30)
        if self._fatal_error is not None:
            raise self._fatal_error
        if cast(IIIConnectionState, self._connection_state) == "failed":
            raise ConnectionError(
                f"Connection to {self._address} failed after max retries"
            )
        if not connected:
            log.warning(
                f"Timed out after 30s waiting for connection to {self._address}; "
                "continuing to retry in background"
            )

    def shutdown(self) -> None:
        """Gracefully shut down the client, releasing all resources.

        Cancels any pending reconnection attempts, rejects all in-flight
        invocations with an error, closes the WebSocket connection, and
        stops the background event-loop thread.  After this call the
        instance must not be reused.

        Examples:
            >>> worker = register_worker('ws://localhost:49134')
            >>> # ... do work ...
            >>> worker.shutdown()
        """
        self._run_on_loop(self.shutdown_async())
        self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(timeout=5)
        self._callbacks.shutdown(wait=False)

    async def connect_async(self) -> None:
        """Connect to the III Engine via WebSocket.

        Initializes OpenTelemetry (if configured), attaches the event loop,
        and establishes the WebSocket connection. This is called automatically
        during construction; use it only if you need to reconnect manually
        from an async context.
        """
        self._running = True
        from iii_helpers.observability.telemetry import attach_event_loop, init_otel

        loop = asyncio.get_running_loop()
        otel_cfg = OtelConfig()
        if self._options.otel:
            if isinstance(self._options.otel, OtelConfig):
                otel_cfg = self._options.otel
            else:
                otel_cfg = OtelConfig(**self._options.otel)
        # Telemetry dials the same engine with the same headers, unless the caller set its own.
        # Headers use `is None`: an explicit `{}` selects no headers and must not inherit the
        # main connection's (which carry III_TOKEN). A blank URL is not a selection, so it keeps `or`.
        otel_cfg = replace(
            otel_cfg,
            engine_ws_url=otel_cfg.engine_ws_url or self._address,
            engine_ws_headers=(
                resolve_headers(self._options.headers)
                if otel_cfg.engine_ws_headers is None
                else otel_cfg.engine_ws_headers
            ),
        )
        init_otel(config=otel_cfg, loop=loop)
        attach_event_loop(loop)
        self._set_connection_state("connecting")
        await self._do_connect()

    async def shutdown_async(self) -> None:
        """Gracefully shut down the client, releasing all resources.

        Cancels any pending reconnection attempts, rejects all in-flight
        invocations with an error, closes the WebSocket connection, and
        stops the background event-loop thread.  After this call the
        instance must not be reused.

        Examples:
            >>> worker = register_worker('ws://localhost:49134')
            >>> # ... do work ...
            >>> await worker.shutdown_async()
        """
        self._running = False
        self._remove_process_marker()

        for task in [self._reconnect_task, self._receiver_task]:
            if task:
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass

        # Last chance for buffered reports and counters to reach the engine.
        try:
            self._reporter.flush()
            self._reporter.send_stats()
        except Exception:
            log.exception("Failed to flush observed invocations on shutdown")
        self._reporter.stop()
        sends = [t for t in self._frame_sends if not t.done()]
        if sends:
            await asyncio.wait(sends, timeout=2)

        # Reject all pending invocations
        for invocation_id, pending in list(self._pending.items()):
            if not pending.future.done():
                pending.future.set_exception(
                    InvocationError(
                        code="SHUTDOWN",
                        message="iii is shutting down",
                        function_id=pending.function_id,
                        invocation_id=invocation_id,
                    )
                )
        self._pending.clear()

        if self._ws:
            await self._ws.close()
            self._ws = None

        self._set_connection_state("disconnected")

        from iii_helpers.observability.telemetry import shutdown_otel_async

        await shutdown_otel_async()

        # Schedule the event loop to stop on the next iteration so the
        # non-daemon background thread exits and the process can terminate.
        self._loop.call_soon(self._loop.stop)

    async def _do_connect(self) -> None:
        ws: ClientConnection | None = None
        try:
            log.debug(f"Connecting to {self._address}")
            ws = await websockets.connect(
                self._address,
                additional_headers=resolve_headers(self._options.headers),
                max_size=_MAX_JSON_MESSAGE_BYTES,
            )
            self._ws = ws
            log.info(f"Connected to {self._address}")
            await self._on_connected()
        except Exception as e:
            # Catch everything: a connect-time failure must never kill the
            # reconnect loop. Enumerating exception types here has failed
            # twice: ConnectionError/OSError missed websockets'
            # InvalidHandshake (MOT-3931), and that in turn missed
            # InvalidMessage, raised when the engine-boot listener race
            # surfaces through iii-network's accept-then-dial as an EOF
            # between TCP connect and the WS 101 (MOT-3966): the exception
            # escaped, the connect task died silently, and the worker stayed
            # a zombie with zero retries. CancelledError is BaseException,
            # so shutdown cancellation still propagates.
            log.warning(f"Connection failed: {type(e).__name__}: {e}")
            if ws is not None and self._ws is ws:
                # The socket failed after the handshake, before the receive
                # loop started (e.g. a proxy reload closed it while the
                # registrations were replayed). Keeping it in `_ws` would end
                # the reconnect loop (`while not self._ws`) with no receiver:
                # the same zombie as iii-hq/iii#2180.
                self._ws = None
                if self._fatal_error is None:
                    self._set_connection_state("disconnected")
                try:
                    await ws.close()
                except Exception as close_error:
                    log.debug(f"Closing the failed socket raised: {close_error!r}")
            if self._running:
                self._schedule_reconnect()

    def _schedule_reconnect(self) -> None:
        if not self._reconnect_task or self._reconnect_task.done():
            self._reconnect_task = asyncio.create_task(self._reconnect_loop())

    async def _reconnect_loop(self) -> None:
        config = self._reconnection_config
        while self._running and not self._ws:
            if (
                config.max_retries != -1
                and self._reconnect_attempt >= config.max_retries
            ):
                self._set_connection_state("failed")
                log.error(
                    f"Max reconnection retries ({config.max_retries}) reached, giving up"
                )
                return

            exponential_delay = config.initial_delay_ms * (
                config.backoff_multiplier**self._reconnect_attempt
            )
            capped_delay = min(exponential_delay, config.max_delay_ms)
            jitter = capped_delay * config.jitter_factor * (2 * random.random() - 1)
            delay_ms = max(0, capped_delay + jitter)

            self._set_connection_state("reconnecting")
            log.debug(
                f"Reconnecting in {delay_ms:.0f}ms (attempt {self._reconnect_attempt + 1})"
            )

            await asyncio.sleep(delay_ms / 1000.0)
            self._reconnect_attempt += 1
            await self._do_connect()

    async def _on_connected(self) -> None:
        self._connection_epoch += 1
        self._registered_on_connection = False
        self._set_connection_state("connected")
        # Reconnect: present the previous engine-assigned identity BEFORE the
        # registration replay so the engine retires the old connection and the
        # replay lands on a clean slate instead of racing its cleanup. The
        # token proves we ARE that worker (ids alone are publicly listable).
        if self._worker_id:
            reattach: dict[str, Any] = {
                "type": MessageType.REATTACH.value,
                "previous_worker_id": self._worker_id,
            }
            if self._reattach_token:
                reattach["reattach_token"] = self._reattach_token
            await self._send(reattach)
        # Re-register (snapshot to avoid mutation from caller thread). A
        # function that needs observe.v2 waits for this connection's
        # `WorkerRegistered` (_on_engine_features).
        for function_id, function_data in list(self._functions.items()):
            replayed = cast(FunctionObservers, function_data.observe)
            if self._needs_v2(function_id, function_data) or replayed.sent_epoch == self._connection_epoch:
                continue
            replayed.sent_epoch = self._connection_epoch
            await self._send(function_data.message)

        # Flush queue (swap to avoid O(n^2) pop(0))
        pending, self._queue = self._queue, []
        for queued_msg in pending:
            if queued_msg.get("type") in _OBSERVE_FAMILY:
                # Sent once `WorkerRegistered` says this engine supports observe.
                self._held_observe_frames.append(queued_msg)
                continue
            if self._ws:
                try:
                    # Replay must surface a dead transport to _do_connect;
                    # _send would requeue on a non-OPEN socket and let setup
                    # falsely succeed, resetting the retry budget forever.
                    await self._send_json(self._ws, self._prepare_json(queued_msg))
                except InvocationError as error:
                    if error.code != "payload_too_large":
                        raise
                    log.warning("Queued JSON rejected: %s", error)
                    invocation_id = queued_msg.get("invocation_id")
                    pending_call = (
                        self._pending.pop(invocation_id, None)
                        if isinstance(invocation_id, str)
                        else None
                    )
                    if pending_call and not pending_call.future.done():
                        pending_call.future.set_exception(error)

        # Register worker metadata
        self._register_worker_metadata()

        self._receiver_task = asyncio.create_task(self._receive_loop())
        # Reset the backoff only once setup succeeded: a failure inside
        # _on_connected counts as an attempt (and against max_retries). A
        # connection that closes after setup restarts the backoff, as in the
        # Node SDK.
        self._reconnect_attempt = 0

    async def _receive_loop(self) -> None:
        ws = self._ws
        if not ws:
            return
        try:
            async for msg in ws:
                try:
                    await self._handle_message(msg)
                except Exception:
                    # One bad frame must not end the loop: the socket would stay
                    # open with nobody reading it, so the worker keeps reporting
                    # `connected` while it never answers again. The Node SDK
                    # logs and drops unparseable frames; this also contains
                    # errors raised while dispatching a parsed message.
                    log.exception("Failed to handle incoming message")
        except websockets.ConnectionClosed:
            pass
        # Reached on every close. A normal close (1000/1001, e.g. an engine
        # restart seen through a reverse proxy) ends `async for` quietly,
        # because websockets swallows ConnectionClosedOK; an abnormal one
        # raises ConnectionClosed above (iii-hq/iii#2180). Not a `finally`:
        # shutdown cancels this task and must still find `_ws` to close it.
        if self._ws is not ws:
            # A stale loop: shutdown or a newer connection already owns `_ws`.
            log.debug("Stale connection closed")
            return
        self._ws = None
        # Local dispatch and the active observer sets survive; what was sent on
        # this connection is replayed on the next one.
        self._registered_on_connection = False
        # INFO with the close code: a proxy-initiated close (e.g. 1001 on a
        # reload) followed by a reconnect should be visible without debug logs.
        log.info(
            f"Connection closed (code={getattr(ws, 'close_code', None)}, "
            f"reason={getattr(ws, 'close_reason', None)!r})"
        )
        # A fatal registration rejection already set the terminal `failed`
        # state and cleared `_running`; the socket close it triggers must not
        # regress the state back to `disconnected`.
        if self._fatal_error is None:
            self._set_connection_state("disconnected")
        if self._running:
            self._schedule_reconnect()

    # Message handling

    def _to_dict(self, msg: Any) -> dict[str, Any]:
        if isinstance(msg, dict):
            return msg
        if hasattr(msg, "model_dump"):
            data: dict[str, Any] = msg.model_dump(by_alias=True, exclude_none=True)
            if "type" in data and hasattr(data["type"], "value"):
                data["type"] = data["type"].value
            if (
                isinstance(msg, InvocationResultMessage)
                and "result" in msg.model_fields_set
            ):
                data.setdefault("result", None)
            return data
        return {"data": msg}

    def _prepare_json(self, msg: Any) -> str:
        data = self._to_dict(msg)
        payload = _wire_json.dumps(data)
        size = len(payload.encode("utf-8"))
        if size <= _MAX_JSON_OUTBOUND_BYTES:
            return payload
        invocation_id = data.get("invocation_id")
        function_id = data.get("function_id")
        correlated_call = (
            data.get("type") == MessageType.INVOKE_FUNCTION.value
            and isinstance(invocation_id, str)
        )
        error = InvocationError(
            code="payload_too_large",
            invocation_id=invocation_id if correlated_call else None,
            function_id=(
                function_id if correlated_call and isinstance(function_id, str) else None
            ),
            message=(
                f"Serialized JSON envelope is {size} bytes; limit is "
                f"{_MAX_JSON_OUTBOUND_BYTES} bytes. Use channels for larger payloads: "
                "https://iii.dev/docs/creating-workers/channels#payload-size"
            ),
        )
        if data.get("type") == MessageType.INVOCATION_RESULT.value:
            fallback = _wire_json.dumps(
                {
                    "type": MessageType.INVOCATION_RESULT.value,
                    "invocation_id": data.get("invocation_id"),
                    "function_id": data.get("function_id"),
                    "error": {"code": error.code, "message": error.message},
                }
            )
            if len(fallback.encode("utf-8")) <= _MAX_JSON_OUTBOUND_BYTES:
                return fallback
        raise error

    async def _send_json(self, ws: ClientConnection, payload: str) -> None:
        """Send a JSON message as one frame or a sequence of fragments."""
        encoded = payload.encode("utf-8")
        log.debug("Send JSON envelope: %d bytes", len(encoded))
        if len(encoded) <= _MAX_JSON_FRAME_BYTES:
            await ws.send(payload)
        else:
            started = False

            def fragments() -> Iterator[str]:
                nonlocal started
                parts = iter(_json_fragments(encoded))
                try:
                    first = next(parts)
                except StopIteration:
                    return
                # websockets consumes the iterable only after waiting for any
                # previous fragmented send, so this marks this message itself.
                started = True
                yield first
                yield from parts

            try:
                await ws.send(fragments())
            except asyncio.CancelledError:
                protocol = getattr(ws, "protocol", None)
                incomplete = getattr(protocol, "expect_continuation_frame", False)
                if started and incomplete:
                    # Only this message may leave this flag true: another send
                    # cannot start until websockets completes the current one.
                    # Abort to avoid leaving an OPEN connection mid-message.
                    transport = getattr(ws, "transport", None)
                    if transport is not None:
                        transport.abort()
                raise

    async def _send(self, msg: Any) -> None:
        payload = self._prepare_json(msg)
        if self._ws and self._ws.state.name == "OPEN":
            await self._send_json(self._ws, payload)
        else:
            if len(self._queue) >= MAX_QUEUE_SIZE:
                log.warning("Message queue full, dropping oldest message")
                self._queue.pop(0)
            self._queue.append(json.loads(payload))

    def _enqueue(self, msg: Any) -> None:
        data = json.loads(self._prepare_json(msg))
        if len(self._queue) >= MAX_QUEUE_SIZE:
            log.warning("Message queue full, dropping oldest message")
            self._queue.pop(0)
        self._queue.append(data)

    def _send_if_connected(self, msg: Any) -> None:
        self._prepare_json(msg)
        if not (self._ws and self._ws.state.name == "OPEN"):
            return
        self._schedule_on_loop(self._send(msg))

    @staticmethod
    def _log_task_exception(task: asyncio.Task[Any]) -> None:
        if task.cancelled():
            return
        exc = task.exception()
        if exc:
            if isinstance(exc, _TraceContextError) and exc.__cause__:
                exc = exc.__cause__
            log.error(f"Error in fire-and-forget send: {exc}")

    async def _handle_message(self, raw: str | bytes) -> None:
        data = json.loads(raw if isinstance(raw, str) else raw.decode())
        msg_type = data.get("type")
        log.debug(f"Recv: {msg_type}")

        if msg_type == MessageType.INVOCATION_RESULT.value:
            self._handle_result(
                data.get("invocation_id", ""),
                data.get("result"),
                data.get("error"),
            )
        elif msg_type == MessageType.INVOKE_FUNCTION.value:
            asyncio.create_task(
                self._handle_invoke(
                    data.get("invocation_id"),
                    data.get("function_id", ""),
                    data.get("data"),
                    data.get("traceparent"),
                    data.get("baggage"),
                    data.get("metadata"),
                    caller=decode_caller(data.get("caller")),
                    observe_depth=decode_observe_depth(data.get("observe_depth")),
                    activation=decode_activation(data.get("activation")),
                )
            )
        elif msg_type == MessageType.FUNCTION_REGISTERED.value:
            registered = FunctionRegisteredMessage.model_validate(data)
            state = self._observe_state(registered.id)
            if registered.id in self._standing_by:
                self._standing_by.discard(registered.id)
                log.info("Function %r is hosted by this worker now (standby)", registered.id)
            if state is not None:
                state.on_function_registered(
                    registered.namespace,
                    registered.revision,
                    registered.local_dispatch,
                    registered.observers,
                    self._connection_epoch,
                )
            # Reports held for this function across a reconnect can go now.
            self._reporter.flush()
        elif msg_type == MessageType.OBSERVER_ADDED.value:
            added = ObserverAddedMessage.model_validate(data)
            state = self._observe_state(added.function_id)
            if state is not None:
                state.on_observer_added(added.namespace, added.revision, added.observer)
        elif msg_type == MessageType.OBSERVER_REMOVED.value:
            removed = ObserverRemovedMessage.model_validate(data)
            state = self._observe_state(removed.function_id)
            if state is not None:
                state.on_observer_removed(removed.revision, removed.id)
        elif msg_type == MessageType.OBSERVE_RESULT.value:
            result = ObserveResultMessage.model_validate(data)
            if result.warnings:
                log.warning("Observer %s: %s", result.id, "; ".join(result.warnings))
            self._on_observer_state(result.id, result.state, result.error.model_dump() if result.error else None)
        elif msg_type == MessageType.OBSERVER_STATE_CHANGED.value:
            changed = ObserverStateChangedMessage.model_validate(data)
            self._on_observer_state(changed.id, changed.state, changed.reason.model_dump() if changed.reason else None)
        elif msg_type == MessageType.WORKER_REGISTERED.value:
            worker = WorkerRegisteredMessage.model_validate(data)
            self._worker_id = worker.worker_id
            self._reattach_token = worker.reattach_token
            log.debug(f"Worker registered with ID: {worker.worker_id}")
            self._on_engine_features(worker.features or [])
        elif msg_type == MessageType.REGISTRATION_REJECTED.value:
            self._handle_registration_rejected(data)

    def _handle_registration_rejected(self, data: dict[str, Any]) -> None:
        """Handle a ``registrationrejected`` push from the engine.

        Codes with different semantics arrive here:

        * ``FUNCTION_NAMESPACE_CONFLICT`` -- another live worker already exports
          this one function id. Only that registration is refused; the engine
          keeps the connection open and this worker keeps serving everything
          else. Non-fatal: log and continue. A function registered with
          ``standby=True`` retries every second (±20 %) until the id is free.
        * ``INVALID_ARGS_FORMAT``, ``UNSUPPORTED`` -- the engine refused one
          function's registration (an ``args_format`` that is not a JSON
          Schema, an option it does not support). Non-fatal, like the above.
        * ``WORKER_NAMESPACE_CONFLICT`` -- another live worker already holds this
          ``(namespace, worker_name)``. The engine closes the connection. Fatal:
          stop and do NOT reconnect. Any unknown code is treated as fatal too.
        """
        rejected = RegistrationRejectedMessage.model_validate(data)
        code = rejected.code

        if code in _FUNCTION_REJECTIONS and rejected.function_id:
            function_id = rejected.function_id
            fn = self._functions.get(function_id)
            # The engine refused it: no local dispatch, no observers for it here.
            state = self._observe_state(function_id)
            if state is not None:
                state.on_registration_rejected(self._connection_epoch)
            if code == "FUNCTION_NAMESPACE_CONFLICT" and fn is not None and fn.standby:
                if function_id not in self._standing_by:
                    self._standing_by.add(function_id)
                    log.info(
                        "Function %r is hosted by worker %r; standing by to host it when that worker leaves",
                        function_id,
                        rejected.owner_worker_id,
                    )
                delay = self._standby_retry_s * random.uniform(0.8, 1.2)
                self._loop.call_later(delay, self._retry_standby, fn, self._connection_epoch)
                return
            if code == "FUNCTION_NAMESPACE_CONFLICT":
                log.warning(
                    "Function registration rejected (%s): namespace=%r "
                    "function_id=%r already owned by worker %r. Skipping that "
                    "function; the worker keeps serving its other exports.",
                    code,
                    rejected.namespace,
                    function_id,
                    rejected.owner_worker_id,
                )
            else:
                log.error(
                    "Function registration rejected (%s): namespace=%r function_id=%r. Skipping that "
                    "function; the worker keeps serving its other exports.",
                    code,
                    rejected.namespace,
                    function_id,
                )
            return

        error = RegistrationRejectedError(
            code=code,
            namespace=rejected.namespace,
            worker_name=rejected.worker_name or "",
            owner_worker_id=rejected.owner_worker_id,
        )
        log.error(
            "Worker registration rejected (%s): namespace=%r worker_name=%r "
            "owner_worker_id=%r. Fatal; not reconnecting.",
            error.code,
            error.namespace,
            error.worker_name,
            error.owner_worker_id,
        )
        self._fatal_error = error
        # Stop first so the receive loop's ConnectionClosed handler (and any
        # pending reconnect loop) sees `_running is False` and gives up.
        self._running = False
        if self._reconnect_task and not self._reconnect_task.done():
            self._reconnect_task.cancel()
        self._set_connection_state("failed")
        # Fail in-flight invocations; they will never resolve on a dead link.
        for invocation_id, pending in list(self._pending.items()):
            if not pending.future.done():
                pending.future.set_exception(error)
        self._pending.clear()
        if self._ws:
            asyncio.create_task(self._ws.close())

    # observe: connection and engine state

    def _observe_state(self, function_id: str) -> FunctionObservers | None:
        fn = self._functions.get(function_id)
        state = fn.observe if fn is not None else None
        return state if isinstance(state, FunctionObservers) else None

    def _is_open(self) -> bool:
        return self._ws is not None and self._ws.state.name == "OPEN"

    def _observe_online(self) -> bool:
        """Connected to an engine that advertised ``observe.v1``."""
        return self._is_open() and self._registered_on_connection and FEATURE_OBSERVE_V1 in self._engine_features

    def _delivers_v2(self) -> bool:
        """This connection's engine advertised ``observe.v2``: the host
        delivers its routed calls, and reports carry ``output_is_input``."""
        return self._registered_on_connection and FEATURE_OBSERVE_V2 in self._engine_features

    def _on_engine_features(self, features: list[str]) -> None:
        """``WorkerRegistered`` told this connection what the engine supports.

        With ``observe.v1`` the held observe frames and the observers are
        sent (after the functions, replayed on connect) and held reports
        flushed; without it ``observe()`` fails with ``UNSUPPORTED``, observer
        hooks are inactive, and held observe frames are dropped. With
        ``observe.v2`` the functions that need it are registered; without it
        they stay unregistered, each logged ``UNSUPPORTED`` once.
        """
        self._engine_features = frozenset(features)
        self._registered_on_connection = True
        for function_id in list(self._functions):
            self._sync_registration(function_id)
        self._schedule_hook_id_check()
        held, self._held_observe_frames = self._held_observe_frames, []
        if FEATURE_OBSERVE_V1 in self._engine_features:
            for frame in held:
                self._spawn(self._send(frame))
            for handle in list(self._observers.values()):
                self._send_observe(handle)
            self._reporter.flush()
            return
        for handle in list(self._observers.values()):
            handle._set_state("error", _UNSUPPORTED_REASON)
        for function_id in list(self._functions):
            if self._has_observer_hooks(function_id):
                self._warn_hooks_inactive(function_id)
        self._reporter.clear()

    def _has_observer_hooks(self, function_id: str) -> bool:
        return (
            self._worker_added is not None
            or self._worker_removed is not None
            or function_id in self._added_hooks
            or function_id in self._removed_hooks
            or function_id in self._match_hooks
        )

    def _warn_hooks_inactive(self, function_id: str) -> None:
        if function_id in self._hooks_inactive_warned:
            return
        self._hooks_inactive_warned.add(function_id)
        log.warning(
            "Function %r has observer hooks, but the engine does not support observe (observe.v1): "
            "its observer hooks are inactive.",
            function_id,
        )

    def _schedule_hook_id_check(self) -> None:
        """SDK loop: names the hook ids this worker has not registered
        ``_hook_id_check_delay_s`` from now, once the connection is registered.
        A later call pushes the check back, so it runs after the setup code
        that sets hooks and registers functions."""
        if not self._registered_on_connection:
            return  # this connection's WorkerRegistered schedules it
        if self._hook_id_check is not None:
            self._hook_id_check.cancel()
        self._hook_id_check = self._loop.call_later(self._hook_id_check_delay_s, self._warn_unregistered_hook_ids)

    def _hook_set_by_id(self) -> None:
        """A hook was set by function id (any thread): the unregistered-id check runs after it."""
        try:
            self._loop.call_soon_threadsafe(self._schedule_hook_id_check)
        except RuntimeError:
            pass  # shut down

    def _warn_unregistered_hook_ids(self) -> None:
        """Once per id: observer hooks set by id for a function this worker has not registered."""
        self._hook_id_check = None
        ids = set(self._added_hooks) | set(self._removed_hooks) | set(self._match_hooks) | set(self._deliver_hooks)
        for function_id in sorted(ids - set(self._functions) - self._hook_ids_warned):
            self._hook_ids_warned.add(function_id)
            log.warning(
                "Observer hooks are set for %r, which this worker has not registered; check the function id.",
                function_id,
            )

    # registration: what an engine without observe.v2 cannot host waits or stays out

    def _needs_v2(self, function_id: str, data: RemoteFunctionData) -> bool:
        """``args_format``, ``private``, ``observable=False`` or an
        ``on_observer_deliver``: the function needs an engine with ``observe.v2``."""
        return data.needs_v2 or function_id in self._deliver_hooks

    def _schedule_registration(self, function_id: str) -> None:
        try:
            self._loop.call_soon_threadsafe(self._sync_registration, function_id)
        except RuntimeError:
            pass  # shut down

    def _sync_registration(self, function_id: str) -> None:
        """SDK loop: sends a function's ``registerfunction`` on the current
        connection when it may go, and keeps a function that needs
        ``observe.v2`` off an engine without it, released there (no local
        dispatch, no observers)."""
        data = self._functions.get(function_id)
        if data is None or not self._is_open():
            return  # replayed on the next connection
        state = cast(FunctionObservers, data.observe)
        epoch = self._connection_epoch
        if not self._needs_v2(function_id, data) or (
            self._registered_on_connection and FEATURE_OBSERVE_V2 in self._engine_features
        ):
            if state.sent_epoch != epoch and self._send_frame(self._prepare_json(data.message)):
                state.sent_epoch = epoch
            return
        if not self._registered_on_connection:
            return  # held until this connection's WorkerRegistered
        if function_id not in self._unsupported_warned:
            self._unsupported_warned.add(function_id)
            log.error(
                "Function %r needs observe.v2 (args_format, private, observable=False or on_observer_deliver), "
                "which the engine does not support (UNSUPPORTED): it stays unregistered. Upgrade the engine to "
                "0.26 or later.",
                function_id,
            )
        if state.sent_epoch == epoch:
            # Registered as a plain function before it set on_observer_deliver.
            self._send_frame(self._prepare_json(UnregisterFunctionMessage(id=function_id)))
        if state.released_epoch != epoch:
            # Unregistered on this connection: no local dispatch, no observers,
            # also when an earlier engine had registered it.
            state.on_registration_rejected(epoch)

    def _retry_standby(self, data: RemoteFunctionData, epoch: int) -> None:
        """SDK loop: a standby registration again, unless the function was
        unregistered or the connection changed (its replay registers it)."""
        function_id = data.message.id
        if self._functions.get(function_id) is not data or epoch != self._connection_epoch:
            return
        log.debug("Retrying the standby registration of %r", function_id)
        self._sync_registration(function_id)

    def _on_observer_state(self, observer_id: str, wire_state: str, reason: dict[str, Any] | None) -> None:
        handle = self._observers.get(observer_id)
        if handle is None:
            return
        state = to_observer_state(wire_state)
        body = decode_error(reason)
        if state == "disposed":
            # Forgotten first: it is not replayed once anyone sees it disposed.
            self._observers.pop(observer_id, None)
        handle._set_state(state, cast(ObserveReason, body) if body else None)

    def _send_observe(self, handle: ObserverHandle) -> None:
        """Sends (or replays) an observer on the SDK loop, when the socket is open."""
        if self._send_frame(self._prepare_json(handle._message)):
            handle._sent = True

    def _after_observe(self, handle: ObserverHandle) -> None:
        """SDK loop: a new observer goes out now if this connection is registered."""
        if self._observers.get(handle.id) is not handle or not self._registered_on_connection or not self._is_open():
            return
        if FEATURE_OBSERVE_V1 in self._engine_features:
            self._send_observe(handle)
        else:
            handle._set_state("error", _UNSUPPORTED_REASON)

    def _dispose_observer(self, handle: ObserverHandle) -> None:
        if self._observers.get(handle.id) is handle:
            del self._observers[handle.id]
        if not handle._sent:
            return

        async def unobserve() -> None:
            # Queued while disconnected: a reattach would otherwise keep it alive.
            if not self._is_open() or FEATURE_OBSERVE_V1 in self._engine_features:
                await self._send(UnobserveMessage(id=handle.id))

        self._schedule_on_loop(unobserve())

    def _send_frame(
        self,
        data: str,
        on_failure: Callable[[BaseException], None] | None = None,
        on_sent: Callable[[], None] | None = None,
    ) -> bool:
        """SDK loop: sends one prepared JSON frame when the socket is open.
        Its bytes count as unsent until the socket took them; ``on_failure``
        runs if the write fails, ``on_sent`` once it succeeded."""
        ws = self._ws
        if ws is None or ws.state.name != "OPEN":
            return False
        size = len(data.encode("utf-8", "surrogatepass"))
        self._inflight_bytes += size

        async def send() -> None:
            try:
                await self._send_json(ws, data)
            except Exception as error:
                if on_failure is not None:
                    on_failure(error)
                raise
            finally:
                self._inflight_bytes -= size
            if on_sent is not None:
                on_sent()

        task = asyncio.ensure_future(send())
        self._frame_sends.add(task)
        task.add_done_callback(self._frame_sends.discard)
        task.add_done_callback(self._background_done)
        return True

    def _unsent_bytes(self) -> int:
        """Frames handed to the socket but not written yet: sends still waiting,
        and what the transport buffers for a peer that does not read."""
        unsent = self._inflight_bytes
        ws = self._ws
        transport = getattr(ws, "transport", None) if ws is not None else None
        if transport is not None:
            try:
                unsent += transport.get_write_buffer_size()
            except Exception:
                pass
        return unsent

    def _spawn(self, coro: Coroutine[Any, Any, Any]) -> None:
        """SDK loop: runs a coroutine nobody awaits, keeping it alive until it ends."""
        task = asyncio.ensure_future(coro)
        self._background.add(task)
        task.add_done_callback(self._background_done)

    def _background_done(self, task: asyncio.Future[Any]) -> None:
        self._background.discard(task)
        if task.cancelled():
            return
        error = task.exception()
        if error is not None and not isinstance(error, websockets.ConnectionClosed):
            if isinstance(error, _TraceContextError) and error.__cause__ is not None:
                error = error.__cause__
            log.error("Error in background task: %s", error)

    async def _on_sdk_loop(self, coro: Coroutine[Any, Any, TResult]) -> TResult:
        """Awaits a coroutine that must run on the SDK loop from any loop; the
        current context (invocation, OpenTelemetry) travels with it."""
        if asyncio.get_running_loop() is self._loop:
            return await coro
        return await asyncio.wrap_future(asyncio.run_coroutine_threadsafe(coro, self._loop))

    def _run_callback(self, callback: Callable[[], None]) -> None:
        """Runs a user callback on the callback thread, off the event loop."""
        try:
            self._callbacks.submit(callback)
        except RuntimeError:
            callback()  # shut down: deliver it here

    def _worker_name(self) -> str:
        # III_WORKER_NAME carries the orchestrator-assigned name (set by
        # iii-worker for engine-managed workers). The engine matches live
        # registrations by name, so that identity must win over a name
        # embedded in worker code.
        return os.environ.get("III_WORKER_NAME") or self._options.worker_name or f"{platform.node()}:{os.getpid()}"

    def _outgoing_observe_fields(self) -> tuple[dict[str, Any] | None, int | None]:
        """``caller`` and ``observe_depth`` an outgoing call carries from the
        current handler (engines with ``observe.v1`` only)."""
        cur = current_context()
        if cur is None or FEATURE_OBSERVE_V1 not in self._engine_features:
            return None, None
        return caller_for(cur), (cur.observe_depth or None)

    def _handle_result(self, invocation_id: str, result: Any, error: Any) -> None:
        pending = self._pending.pop(invocation_id, None)
        if not pending:
            log.debug(f"No pending invocation: {invocation_id}")
            return

        if pending.future.done():
            # The caller stopped waiting (its await was cancelled); settling a
            # done future raises InvalidStateError.
            log.debug(f"Dropping result for settled invocation: {invocation_id}")
            return

        if error:
            pending.future.set_exception(
                _wrap_wire_error(
                    error,
                    function_id=pending.function_id,
                    invocation_id=invocation_id,
                )
            )
        else:
            pending.future.set_result(result)

    def _inject_traceparent(self) -> str | None:
        from opentelemetry import context as otel_context
        from opentelemetry import propagate

        carrier: dict[str, str] = {}
        propagate.inject(carrier, context=otel_context.get_current())
        return carrier.get("traceparent")

    def _inject_baggage(self) -> str | None:
        from opentelemetry import context as otel_context
        from opentelemetry import propagate

        carrier: dict[str, str] = {}
        propagate.inject(carrier, context=otel_context.get_current())
        return carrier.get("baggage")

    @contextmanager
    def _execute_span(
        self, function_id: str, data: Any, traceparent: str | None, baggage: str | None
    ) -> Iterator[_SpanOutcome]:
        """The ``execute <fn>`` span around a handler, remote or local. The
        body sets ``outcome.result``; a raised exception is recorded and comes
        out as :class:`_TraceContextError` (its cause is the handler's error)."""
        from opentelemetry import context as otel_context
        from opentelemetry import propagate, trace

        carrier: dict[str, str] = {}
        if traceparent:
            carrier["traceparent"] = traceparent
        if baggage:
            carrier["baggage"] = baggage
        parent_ctx = (
            propagate.extract(carrier) if carrier else otel_context.get_current()
        )
        tracer = trace.get_tracer("iii-python-sdk")

        from iii_helpers.observability import redact_and_truncate, resolve_max_bytes_from_env

        trace_payloads = os.environ.get("III_DISABLE_TRACE_PAYLOADS", "").lower() not in (
            "1",
            "true",
        )
        payload_max_bytes = resolve_max_bytes_from_env()

        # INTERNAL and named `execute` (not `call`): the engine already emits
        # the SERVER `call <fn>` span for this hop. Reusing that name would
        # duplicate an engine span under the worker's service. `execute` is
        # unique, so the worker handler span reads as a clean internal child
        # of the engine's call span (and is collapsible by a single rule).
        with tracer.start_as_current_span(
            f"execute {function_id}",
            context=parent_ctx,
            kind=trace.SpanKind.INTERNAL,
        ) as span:
            if trace_payloads and span.is_recording():
                input_json, input_truncated = redact_and_truncate(data, payload_max_bytes)
                span.add_event(
                    "iii.invocation.input",
                    attributes={
                        "iii.payload.json": input_json,
                        "iii.payload.truncated": input_truncated,
                    },
                )
            outcome = _SpanOutcome()
            try:
                yield outcome
            except Exception as e:
                if trace_payloads and span.is_recording():
                    err_json, err_truncated = redact_and_truncate(
                        {"error": str(e)}, payload_max_bytes
                    )
                    span.add_event(
                        "iii.invocation.output",
                        attributes={
                            "iii.payload.json": err_json,
                            "iii.payload.truncated": err_truncated,
                            "iii.payload.ok": False,
                        },
                    )
                span.record_exception(e)
                span.set_status(trace.StatusCode.ERROR, str(e))
                response_traceparent = self._inject_traceparent()
                raise _TraceContextError(response_traceparent) from e
            if trace_payloads and span.is_recording():
                out_json, out_truncated = redact_and_truncate(outcome.result, payload_max_bytes)
                span.add_event(
                    "iii.invocation.output",
                    attributes={
                        "iii.payload.json": out_json,
                        "iii.payload.truncated": out_truncated,
                        "iii.payload.ok": True,
                    },
                )
            span.set_status(trace.StatusCode.OK)
            outcome.traceparent = self._inject_traceparent()

    async def _invoke_with_otel_context(
        self,
        function_id: str,
        handler: Callable[..., Awaitable[Any]],
        data: Any,
        traceparent: str | None,
        baggage: str | None,
        metadata: dict[str, Any] | None = None,
    ) -> tuple[Any, str | None]:
        with self._execute_span(function_id, data, traceparent, baggage) as outcome:
            try:
                outcome.result = await handler(data, metadata)
            except BaseException as error:
                # SystemExit / KeyboardInterrupt from a handler must not escape
                # the task and stop the SDK loop: it fails this invocation.
                guarded = guard_exit(error)
                if guarded is error:
                    raise
                raise guarded from error
        return outcome.result, outcome.traceparent

    def _resolve_channels(self, data: Any) -> Any:
        """``data`` with its channel refs as readers and writers. Containers
        are rebuilt only along the path to a ref: without refs it is ``data``
        itself (a private function's input passes by reference)."""
        if is_channel_ref(data):
            ref = StreamChannelRef(**data)
            return (
                ChannelReader(self._address, ref, self._options.headers)
                if ref.direction == "read"
                else ChannelWriter(self._address, ref, self._options.headers)
            )
        if isinstance(data, dict):
            resolved_dict: dict[Any, Any] | None = None
            for key, value in data.items():
                resolved = self._resolve_channels(value)
                if resolved is not value:
                    if resolved_dict is None:
                        resolved_dict = dict(data)
                    resolved_dict[key] = resolved
            return data if resolved_dict is None else resolved_dict
        if isinstance(data, (list, tuple)):
            items = [self._resolve_channels(v) for v in data]
            if all(new is old for new, old in zip(items, data)):
                return data
            return items if isinstance(data, list) else tuple(items)
        return data

    async def _handle_invoke(
        self,
        invocation_id: str | None,
        path: str,
        data: Any,
        traceparent: str | None = None,
        baggage: str | None = None,
        metadata: dict[str, Any] | None = None,
        *,
        caller: dict[str, Any] | None = None,
        observe_depth: int = 0,
        activation: dict[str, Any] | None = None,
    ) -> None:
        # The invocation context every handler sees (`current_invocation()`),
        # and what the calls it makes carry as their caller. Activations carry
        # an id and are answered like any call.
        token = set_context(
            ActiveInvocation(
                function_id=path,
                invocation_id=invocation_id or str(uuid.uuid4()),
                observe_depth=observe_depth,
                caller=caller,
                activation=activation,
                metadata=metadata,
            )
        )
        try:
            await self._handle_invoke_in_context(invocation_id, path, data, traceparent, baggage, metadata)
        finally:
            reset_context(token)

    async def _handle_invoke_in_context(
        self,
        invocation_id: str | None,
        path: str,
        data: Any,
        traceparent: str | None,
        baggage: str | None,
        metadata: dict[str, Any] | None,
    ) -> None:
        func = self._functions.get(path)

        if not func or not func.handler:
            error_code = "function_not_invokable" if func else "function_not_found"
            if func:
                error_msg = "Function is HTTP-invoked and cannot be invoked locally"
            else:
                error_msg = f"Function '{path}' not found"
            log.warning(error_msg)
            if invocation_id:
                await self._send(
                    InvocationResultMessage(
                        invocation_id=invocation_id,
                        function_id=path,
                        error={"code": error_code, "message": error_msg},
                    )
                )
            return

        try:
            resolved_data = self._resolve_channels(data)
        except Exception as e:
            log.exception("Failed to resolve channel refs")
            if invocation_id:
                await self._send(
                    InvocationResultMessage(
                        invocation_id=invocation_id,
                        function_id=path,
                        error={
                            "code": "invocation_failed",
                            "message": str(e),
                            "stacktrace": traceback.format_exc(),
                        },
                    )
                )
            return

        if not invocation_id:
            task = asyncio.create_task(
                self._invoke_with_otel_context(
                    path, func.handler, resolved_data, traceparent, baggage, metadata
                )
            )
            task.add_done_callback(self._log_task_exception)
            return

        # The host delivers a routed call when the engine advertised observe.v2;
        # otherwise the engine evaluates it. The call is matched against the
        # observers active when it starts.
        if self._delivers_v2():
            state = cast(FunctionObservers, func.observe)
            active = state.active
            hook = self._deliver_hooks.get(path)
            if hook is not None or active.observers:
                await self._invoke_routed_and_deliver(
                    invocation_id,
                    path,
                    func,
                    state,
                    active,
                    hook,
                    data,
                    resolved_data,
                    traceparent,
                    baggage,
                    metadata,
                )
                return

        try:
            result, response_traceparent = await self._invoke_with_otel_context(
                path,
                func.handler,
                resolved_data,
                traceparent,
                baggage,
                metadata,
            )
            await self._send(
                InvocationResultMessage(
                    invocation_id=invocation_id,
                    function_id=path,
                    result=result,
                    traceparent=response_traceparent,
                )
            )
        except _TraceContextError as te:
            original = te.__cause__
            log.exception(f"Error in handler {path}")
            await self._send(
                InvocationResultMessage(
                    invocation_id=invocation_id,
                    function_id=path,
                    error={
                        "code": "invocation_failed",
                        "message": str(original),
                        "stacktrace": traceback.format_exc(),
                    },
                    traceparent=te.traceparent,
                )
            )
        except Exception as e:
            log.exception(f"Error in handler {path}")
            await self._send(
                InvocationResultMessage(
                    invocation_id=invocation_id,
                    function_id=path,
                    error={
                        "code": "invocation_failed",
                        "message": str(e),
                        "stacktrace": traceback.format_exc(),
                    },
                )
            )

    async def _invoke_routed_and_deliver(
        self,
        invocation_id: str,
        path: str,
        func: RemoteFunctionData,
        state: FunctionObservers,
        active: ActiveSet,
        hook: DeliverHook | None,
        data: Any,
        resolved_data: Any,
        traceparent: str | None,
        baggage: str | None,
        metadata: dict[str, Any] | None,
    ) -> None:
        """A routed call of an observed function (or one with a deliver hook)
        on an ``observe.v2`` engine: the handler, then its delivery, then the
        answer. The handler gets ``resolved_data`` (channel refs as readers
        and writers); its delivery sees ``data``, the input as it arrived.
        The ids iii's delivery selected ride the answer's ``observed``; a
        function with a deliver hook lists none."""
        handler = cast(Callable[..., Awaitable[Any]], func.handler)
        ctx = cast(ActiveInvocation, current_context())
        started_at_ms = time.time() * 1000.0
        t0 = time.perf_counter()
        delivery: Delivery | None = None
        answer: InvocationResultMessage
        try:
            with self._execute_span(path, resolved_data, traceparent, baggage) as outcome:
                output: Any = None
                handler_error: BaseException | None = None
                try:
                    output = await handler(resolved_data, metadata)
                except asyncio.CancelledError:
                    raise
                except BaseException as error:
                    handler_error = error
                    log.error("Error in handler %s", path, exc_info=error)
                delivery = Delivery(
                    state,
                    Call(
                        function_id=path,
                        invocation_id=invocation_id,
                        status="success" if handler_error is None else "error",
                        input=data,
                        output=output,
                        error=_handler_error_body(handler_error) if handler_error is not None else None,
                        caller=ctx.caller,
                        metadata=metadata,
                    ),
                    handler_error,
                    active,
                    routed=True,
                    started_at_ms=started_at_ms,
                    duration_ms=(time.perf_counter() - t0) * 1000.0,
                    observe_depth=ctx.observe_depth,
                )
                ok, value = await delivery.run_async(hook)
                if not ok:
                    raise guard_exit(value)
                outcome.result = value
            answer = InvocationResultMessage(
                invocation_id=invocation_id,
                function_id=path,
                result=value,
                traceparent=outcome.traceparent,
                observed=delivery.observed,
            )
        except _TraceContextError as te:
            failure = original_error(cast(BaseException, te.__cause__))
            own = delivery is not None and failure is delivery.handler_error
            if not own:
                log.error("Error in on_observer_deliver of %s", path, exc_info=failure)
            answer = InvocationResultMessage(
                invocation_id=invocation_id,
                function_id=path,
                error=cast(Delivery, delivery).call.error if own else _answer_error_body(failure),
                traceparent=te.traceparent,
                observed=delivery.observed if delivery is not None else None,
            )
        await self._send(answer)

    # Connection state management

    def _set_connection_state(self, state: IIIConnectionState) -> None:
        if self._connection_state != state:
            self._connection_state = state
            if state == "connected":
                self._connected_event.set()
            else:
                self._connected_event.clear()
            for handler in list(self._connection_listeners):
                try:
                    handler(state)
                except Exception:
                    log.exception("Connection state listener raised")

    def add_connection_state_listener(
        self, handler: ConnectionStateCallback
    ) -> Callable[[], None]:
        """Subscribe to connection-state transitions.

        The handler is fired immediately with the current state (on the
        caller's thread), then once per transition. Transitions fire on the
        SDK's background event-loop thread: keep handlers fast and do not
        call sync SDK methods from them (they would raise ``RuntimeError``).
        Treat calls as state notifications, not edges. A state may rarely
        be observed twice around subscription. Registering the same handler
        twice fires it twice. Returns an idempotent unsubscribe function
        that removes only its own registration.

        Examples:
            >>> unsubscribe = worker.add_connection_state_listener(
            ...     lambda state: print(f"engine link: {state}")
            ... )
        """
        self._connection_listeners.append(handler)
        try:
            handler(self._connection_state)
        except Exception:
            log.exception("Connection state listener raised on initial fire")

        unsubscribed = False

        def unsubscribe() -> None:
            nonlocal unsubscribed
            if unsubscribed:
                return
            unsubscribed = True
            try:
                self._connection_listeners.remove(handler)
            except ValueError:
                pass

        return unsubscribe

    def get_address(self) -> str:
        """Return the engine address this worker resolved to.

        The explicit ``register_worker`` argument, else ``III_URL``, else
        :data:`DEFAULT_ENGINE_URL`. Mirrors the Rust SDK's ``address()`` and the
        Node SDK's ``getAddress()``.
        """
        return self._address

    def get_connection_state(self) -> IIIConnectionState:
        """Return the current WebSocket connection state.

        Returns:
            One of ``"disconnected"``, ``"connecting"``, ``"connected"``,
            ``"reconnecting"``, or ``"failed"``.

        Examples:
            >>> worker = register_worker("ws://localhost:49134")
            >>> if worker.get_connection_state() != "connected":
            ...     print("engine not reachable yet")
        """
        return self._connection_state

    @property
    def worker_id(self) -> str | None:
        """The worker ID assigned by the engine, or None if not yet registered."""
        return self._worker_id

    # Public API

    def register_function(
        self,
        function_id: str,
        handler_or_invocation: RemoteFunctionHandler | HttpInvocationConfig,
        *,
        description: str | None = None,
        metadata: dict[str, Any] | None = None,
        request_format: RegisterFunctionFormat | dict[str, Any] | None = None,
        response_format: RegisterFunctionFormat | dict[str, Any] | None = None,
        args_format: Any = None,
        private: bool = False,
        observable: bool = True,
        standby: bool = False,
        local_dispatch: bool = True,
    ) -> FunctionRef:
        """Register a function with the engine.

        Pass a handler for local execution, or an ``HttpInvocationConfig``
        for HTTP-invoked functions (Lambda, Cloudflare Workers, etc.).

        Handlers can be synchronous or asynchronous.  Sync handlers are
        automatically wrapped with ``run_in_executor`` so they do not
        block the event loop.  Each handler receives a ``data`` argument
        containing the call's payload, and may optionally accept a second
        ``metadata`` argument carrying per-invocation metadata (e.g.
        ``def handler(data, metadata=None)`` or ``def handler(data, *,
        metadata=None)``).  Metadata is only forwarded to handlers that
        declare a parameter literally named ``metadata``, so existing
        handlers, including ones with unrelated extra parameters,
        ``*args``, or ``**kwargs``, keep working unchanged.

        The handler's return value is what the function's observers observe.
        An event source's handler returns the event it is called with. What
        this worker does when the function is observed is set with the hook
        methods (``on_observer_added``, ``on_observer_removed``,
        ``on_observer_match``, ``on_observer_deliver``) on the client by id or
        on the returned ``FunctionRef``. Set ``on_observer_added`` and
        ``on_observer_removed`` by id before registering so they run for every
        observer: the engine may announce an observer before a hook set on the
        ``FunctionRef`` is in place.

        ``request_format`` and ``response_format`` are auto-extracted
        from the handler's type hints when omitted or passed as ``None``
        (the default).  To opt out of auto-extraction, pass an explicit
        schema (``RegisterFunctionFormat`` or ``dict``).  This behavior
        is Python-specific; the Node SDK relies on explicit schemas because
        TypeScript types are erased at runtime.

        Args:
            function_id: Unique string identifier for the function.
            handler_or_invocation: A callable handler or
                ``HttpInvocationConfig``.  Callable handlers receive
                ``data`` (the call's payload) as the first argument and
                may optionally accept ``metadata`` (per-invocation
                metadata) as a second argument; they may return a value.
            description: Human-readable description of what the function does.
            metadata: Arbitrary metadata attached to the function.
            request_format: Schema describing expected input.  When
                ``None`` (default), auto-extracted from the handler's
                first-parameter type hint.  Pass an explicit schema to
                override; there is no way to register with no schema
                when the handler is typed.
            response_format: Schema describing the handler's return value,
                what observers observe.  Same auto-extraction semantics as
                ``request_format``.
            args_format: JSON Schema (or a Pydantic model) of the observers'
                ``args``; the engine validates them (``INVALID_ARGS``). A
                function without it takes no args.
            private: Only this worker may call the function; others may
                observe it. This worker's own calls pass the input and the
                result by reference: the handler and hooks treat both as
                read-only.
            observable: ``False``: nobody may observe the function
                (``NOT_OBSERVABLE``), for functions whose payloads carry
                credentials.
            standby: When another worker already hosts the id
                (``FUNCTION_NAMESPACE_CONFLICT``), retry every second (±20 %)
                and take the function over when that worker leaves.
            local_dispatch: Calls this worker makes to its own function run
                in-process once the engine confirmed it (default ``True``).
                Like a remote invocation, each local call to a sync handler
                runs on a thread of its own (one per call, not pooled), so the
                caller can stop waiting at its timeout. A sync handler that
                blocks for long should be async, or set ``False`` so its calls
                go through the engine (also for a handler that relies on
                engine-side effects).

        A function that declares ``args_format``, ``private`` or
        ``observable=False``, or has an ``on_observer_deliver``, needs an
        engine with ``observe.v2``: against an older engine it stays
        unregistered and ``UNSUPPORTED`` is logged once.

        Returns:
            A ``FunctionRef``: ``id``, the hook methods, ``observers()``,
            ``is_observed()`` and ``unregister()``.

        Raises:
            TypeError: If ``function_id`` is not a string, if
                ``handler_or_invocation`` is not callable or
                ``HttpInvocationConfig``, or if ``args_format`` is not JSON.
            ValueError: If ``function_id`` is empty or already registered.

        Examples:
            >>> def greet(data):
            ...     return {'message': f"Hello, {data['name']}!"}
            >>> fn = worker.register_function("greet", greet, description="Greets a user")
            >>> fn.unregister()

            >>> from pydantic import BaseModel
            >>> class GreetInput(BaseModel):
            ...     name: str
            >>> class GreetOutput(BaseModel):
            ...     message: str
            >>> async def greet(data: GreetInput) -> GreetOutput:
            ...     return GreetOutput(message=f"Hello, {data.name}!")
            >>> fn = worker.register_function("greet", greet, description="Greets a user")

            >>> on_tick = worker.register_function(
            ...     "cron::on-tick", lambda tick: tick, args_format=CronArgs, private=True
            ... )
        """
        if not isinstance(function_id, str):
            raise TypeError(
                f"function_id must be str, got {type(function_id).__name__}"
            )
        if not function_id or not function_id.strip():
            raise ValueError("id is required")
        if function_id in self._functions:
            raise ValueError(f"function id '{function_id}' already registered")

        resolved_args_format = (
            snapshot(_resolve_format(args_format), "args_format") if args_format is not None else None
        )
        handler_for_extraction = (
            handler_or_invocation if callable(handler_or_invocation) else None
        )
        if request_format is None and handler_for_extraction is not None:
            request_format = extract_request_format(handler_for_extraction)
        if response_format is None and handler_for_extraction is not None:
            response_format = extract_response_format(handler_for_extraction)
        func = RegisterFunctionInput(
            id=function_id,
            description=description,
            metadata=metadata,
            request_format=request_format,
            response_format=response_format,
        )
        # Only what the engine reads travels: the defaults (`private: false`,
        # `observable: true`) are left out, and hooks, `standby` and
        # `local_dispatch` stay in this SDK.
        msg = RegisterFunctionMessage(
            id=func.id,
            invocation=handler_or_invocation if isinstance(handler_or_invocation, HttpInvocationConfig) else None,
            description=func.description,
            metadata=func.metadata,
            request_format=func.request_format,
            response_format=func.response_format,
            args_format=resolved_args_format,
            private=True if private else None,
            observable=None if observable else False,
        )
        # Isolate retained JSON fields from later caller mutations.
        msg = msg.model_copy(deep=True)
        self._prepare_json(msg)
        needs_v2 = resolved_args_format is not None or private or not observable

        if isinstance(handler_or_invocation, HttpInvocationConfig):
            observe_state = FunctionObservers(
                function_id, local_dispatch, False, self._observe_host, private=private
            )
            data = RemoteFunctionData(message=msg, observe=observe_state, needs_v2=needs_v2, standby=standby)
        else:
            if not callable(handler_or_invocation):
                actual_type = type(handler_or_invocation).__name__
                raise TypeError(
                    f"handler_or_invocation must be callable or HttpInvocationConfig, got {actual_type}"
                )
            handler = cast("Callable[..., Any]", handler_or_invocation)

            # Decide once, at registration time, whether this handler accepts
            # per-invocation metadata. 1-arg handlers keep working unchanged.
            metadata_mode = _metadata_passing_mode(handler)

            def _call_handler(input_data: Any, metadata: dict[str, Any] | None) -> Any:
                if metadata_mode == "positional":
                    return handler(input_data, metadata)
                if metadata_mode == "keyword":
                    return handler(input_data, metadata=metadata)
                return handler(input_data)

            is_async = is_async_callable(handler)
            if is_async:

                async def wrapped(input_data: Any, metadata: dict[str, Any] | None = None) -> Any:
                    return await _call_handler(input_data, metadata)

            else:

                async def wrapped(input_data: Any, metadata: dict[str, Any] | None = None) -> Any:
                    # A thread of its own, started in a copy of this context:
                    # the current invocation and the OpenTelemetry context
                    # (the `execute` span) follow the handler.
                    return await run_in_thread(_call_handler, input_data, metadata)

            observe_state = FunctionObservers(function_id, local_dispatch, True, self._observe_host, private=private)
            data = RemoteFunctionData(
                message=msg,
                handler=wrapped,
                observe=observe_state,
                call_sync=None if is_async else _call_handler,
                needs_v2=needs_v2,
                standby=standby,
            )
        # Stored before it is sent: the engine's `functionregistered` must find it.
        self._functions[func.id] = data
        self._schedule_registration(func.id)

        if (
            self._has_observer_hooks(function_id)
            and self._registered_on_connection
            and FEATURE_OBSERVE_V1 not in self._engine_features
        ):
            self._warn_hooks_inactive(function_id)

        func_id = func.id

        def unregister() -> None:
            if self._functions.get(func_id) is data:
                del self._functions[func_id]
            self._standing_by.discard(func_id)
            self._send_if_connected(UnregisterFunctionMessage(id=func_id))
            observe_state.dispose()

        return FunctionRef(self, func_id, unregister)

    # observer hooks

    @overload
    def on_observer_added(self, function_id_or_hook: AddedT) -> AddedT: ...
    @overload
    def on_observer_added(self, function_id_or_hook: None) -> None: ...
    @overload
    def on_observer_added(self, function_id_or_hook: str) -> Callable[[AddedT], AddedT]: ...
    @overload
    def on_observer_added(self, function_id_or_hook: str, hook: AddedT) -> AddedT: ...
    @overload
    def on_observer_added(self, function_id_or_hook: str, hook: None) -> None: ...
    def on_observer_added(self, function_id_or_hook: Any, hook: Any = _MISSING) -> Any:
        """Sets what runs when an observer is added, before it is accepted:
        for every function this worker hosts (``on_observer_added(hook)``), or
        for one (``on_observer_added("fn::id", hook)``). Works as a decorator,
        ``@iii.on_observer_added`` or ``@iii.on_observer_added("fn::id")``, and
        returns the hook; ``None`` removes it.

        The worker's hook runs, then the function's; the observer is accepted
        (acked, then active) once both resolved. A hook that raises rejects
        the observer with the :class:`ObserveRejection` it raised, or
        ``OBSERVE_REJECTED``; one still running after 30 s rejects it with
        ``OWNER_TIMEOUT``. Then the ``on_observer_removed`` of each level whose
        hook already resolved runs. Hooks may be sync (on a thread of their
        own) or async, and the hooks of one function run one at a time.

        Hooks are stored by function id, so they can be set before the
        function is registered (which makes them run for every observer) and
        stay in place across ``unregister()``. A hook set while the function
        already has active observers runs for each of them at once (a raise
        rejects that observer) and logs one warning. A second after the
        connection is registered, and after the last hook set by id, a
        warning names each hook id this worker has not registered.

        Examples:
            >>> @iii.on_observer_added            # every function this worker hosts
            ... async def retain(obs):
            ...     await events.retain()
            >>> @iii.on_observer_added("cron::on-tick")
            ... def start(obs):
            ...     scheduler.retain(obs.args["expression"])
        """
        if isinstance(function_id_or_hook, str):
            function_id = function_id_or_hook
            if hook is _MISSING:

                def decorator(fn: AddedT) -> AddedT:
                    self._set_added_hook(function_id, fn)
                    return fn

                return decorator
            self._set_added_hook(function_id, hook)
            return hook
        if hook is not _MISSING:
            raise TypeError("on_observer_added takes a hook, or a function id and a hook")
        self._set_added_hook(None, function_id_or_hook)
        return function_id_or_hook

    @overload
    def on_observer_removed(self, function_id_or_hook: RemovedT) -> RemovedT: ...
    @overload
    def on_observer_removed(self, function_id_or_hook: None) -> None: ...
    @overload
    def on_observer_removed(self, function_id_or_hook: str) -> Callable[[RemovedT], RemovedT]: ...
    @overload
    def on_observer_removed(self, function_id_or_hook: str, hook: RemovedT) -> RemovedT: ...
    @overload
    def on_observer_removed(self, function_id_or_hook: str, hook: None) -> None: ...
    def on_observer_removed(self, function_id_or_hook: Any, hook: Any = _MISSING) -> Any:
        """Sets what runs when an accepted observer goes (on ``unobserve``, on
        disposal, after a late rejection): for every function this worker
        hosts, or for one. The same forms as :meth:`on_observer_added`.

        It runs exactly once for every accepted observer, the function's hook
        and then the worker's, and never for an observer that was not
        accepted. An error it raises is logged; the observer is removed
        either way.

        Examples:
            >>> iii.on_observer_removed(lambda obs: events.release())
            >>> @iii.on_observer_removed("cron::on-tick")
            ... def stop(obs):
            ...     scheduler.release(obs.args["expression"])
        """
        if isinstance(function_id_or_hook, str):
            function_id = function_id_or_hook
            if hook is _MISSING:

                def decorator(fn: RemovedT) -> RemovedT:
                    self._set_removed_hook(function_id, fn)
                    return fn

                return decorator
            self._set_removed_hook(function_id, hook)
            return hook
        if hook is not _MISSING:
            raise TypeError("on_observer_removed takes a hook, or a function id and a hook")
        self._set_removed_hook(None, function_id_or_hook)
        return function_id_or_hook

    @overload
    def on_observer_match(self, function_id: str) -> Callable[[MatchT], MatchT]: ...
    @overload
    def on_observer_match(self, function_id: str, hook: MatchT) -> MatchT: ...
    @overload
    def on_observer_match(self, function_id: str, hook: MatchKeys) -> MatchKeys: ...
    @overload
    def on_observer_match(self, function_id: str, hook: None) -> None: ...
    @overload
    def on_observer_match(
        self, function_id: str, *, observer: Callable[[Observer], Key], call: Callable[[Call[Any, Any]], Key]
    ) -> MatchKeys: ...
    def on_observer_match(
        self, function_id: str, hook: Any = _MISSING, *, observer: Any = None, call: Any = None
    ) -> Any:
        """Sets which observers want each call of one function. By default
        every observer wants every call; iii's delivery then applies their
        ``on`` and filters, and an ``on_observer_deliver`` receives only the
        observers that matched.

        As a predicate, ``hook(obs, call) -> bool`` is asked once per active
        observer per call, before filters; a predicate that raises counts as
        no match. As keys, ``observer=`` and ``call=`` key functions each
        return a string or strings, and an observer wants a call when they
        share a key: the SDK computes an observer's keys once, when it is
        accepted, and files it under each, so a call costs one lookup per key
        whatever the number of observers. An observer whose key function
        raises is rejected (``OBSERVE_REJECTED``); a call whose key function
        raises matches no observer. A match hook that raises is logged once
        per function. Wildcards are keys too: file an observer of every table
        under ``"primary|*|insert"`` and give a change the keys
        ``"primary|orders|insert"``, ``"primary|orders|*"``,
        ``"primary|*|insert"`` and ``"primary|*|*"``.

        Works as a decorator for a predicate
        (``@iii.on_observer_match("fn::id")``) and returns the hook (a
        :class:`MatchKeys` for the keys form); ``None`` removes it. A match
        hook set while the function has active observers is applied to them
        on the SDK loop, which rebuilds the index from them. Predicates and
        key functions are sync: they run on every call.

        Examples:
            >>> iii.on_observer_match(
            ...     "cron::on-tick", lambda obs, call: obs.args["expression"] == call.input["expression"]
            ... )
            >>> iii.on_observer_match(
            ...     "database::on-row-changed",
            ...     observer=lambda obs: f"{obs.args['table']}|{obs.args['op']}",
            ...     call=lambda call: f"{call.input['table']}|{call.input['op']}",
            ... )
        """
        if not isinstance(function_id, str):
            raise TypeError("on_observer_match takes a function id, and a predicate or observer= and call=")
        if observer is not None or call is not None:
            if hook is not _MISSING:
                raise TypeError("on_observer_match takes a predicate or observer= and call= key functions, not both")
            if observer is None or call is None:
                raise TypeError("on_observer_match: keys take both observer= and call=")
            keys = MatchKeys(observer=observer, call=call)
            self._set_match_hook(function_id, keys)
            return keys
        if hook is _MISSING:

            def decorator(fn: MatchT) -> MatchT:
                self._set_match_hook(function_id, fn)
                return fn

            return decorator
        self._set_match_hook(function_id, hook)
        return hook

    @overload
    def on_observer_deliver(self, function_id: str) -> Callable[[DeliverT], DeliverT]: ...
    @overload
    def on_observer_deliver(self, function_id: str, hook: DeliverT) -> DeliverT: ...
    @overload
    def on_observer_deliver(self, function_id: str, hook: None) -> None: ...
    def on_observer_deliver(self, function_id: str, hook: Any = _MISSING) -> Any:
        """Delivers each call of one function itself: ``hook(call,
        observers)`` receives the :class:`Call` and the observers that matched
        it (``on_observer_match``, or all of them), as a snapshot taken when
        the call started, and what it returns (or raises) is what the caller
        gets back. iii delivers nothing for that function: the hook calls
        through observers (``iii.call({"observer": obs, ...})``) and applies
        their ``on`` and filters with ``obs.matches(call)``. Works as a
        decorator (``@iii.on_observer_deliver("fn::id")``) and returns the
        hook; ``None`` restores iii's delivery.

        The hook runs inside the function's invocation, so the calls it makes
        have that invocation as their caller. A sync hook runs on a thread of
        its own. A function with a deliver hook needs an engine with
        ``observe.v2``.

        Examples:
            >>> @iii.on_observer_deliver("http::on-request")
            ... async def route(call, observers):
            ...     obs = routes.match(call.input)
            ...     if obs is None:
            ...         return {"status_code": 404}
            ...     if not obs.matches(call):
            ...         return {"status_code": 422}
            ...     return await iii.call_async({"observer": obs, "payload": call.input})
        """
        if not isinstance(function_id, str):
            raise TypeError("on_observer_deliver takes a function id, and a hook")
        if hook is _MISSING:

            def decorator(fn: DeliverT) -> DeliverT:
                self._set_deliver_hook(function_id, fn)
                return fn

            return decorator
        self._set_deliver_hook(function_id, hook)
        return hook

    def _set_added_hook(self, function_id: str | None, hook: AddedHook | None) -> None:
        if hook is not None and not callable(hook):
            raise TypeError(f"on_observer_added: the hook must be callable, got {type(hook).__name__}")
        if function_id is None:
            self._worker_added = hook
            targets = [cast(FunctionObservers, fn.observe) for fn in list(self._functions.values())]
        else:
            if hook is None:
                self._added_hooks.pop(function_id, None)
            else:
                self._added_hooks[function_id] = hook
                self._hook_set_by_id()
            state = self._observe_state(function_id)
            targets = [state] if state is not None else []
        if hook is not None:
            level: Level = "worker" if function_id is None else "function"
            for state in targets:
                state.catch_up(level)

    def _set_removed_hook(self, function_id: str | None, hook: RemovedHook | None) -> None:
        if hook is not None and not callable(hook):
            raise TypeError(f"on_observer_removed: the hook must be callable, got {type(hook).__name__}")
        if function_id is None:
            self._worker_removed = hook
        elif hook is None:
            self._removed_hooks.pop(function_id, None)
        else:
            self._removed_hooks[function_id] = hook
            self._hook_set_by_id()

    def _set_match_hook(self, function_id: str, hook: MatchHook | None) -> None:
        if hook is None:
            self._match_hooks.pop(function_id, None)
        else:
            if isinstance(hook, MatchKeys):
                functions: tuple[Any, ...] = (hook.observer, hook.call)
            elif callable(hook):
                functions = (hook,)
            else:
                raise TypeError(
                    f"on_observer_match: the hook must be a predicate, MatchKeys or None, got {type(hook).__name__}"
                )
            for fn in functions:
                if not callable(fn):
                    raise TypeError(f"on_observer_match: a key function must be callable, got {type(fn).__name__}")
                if is_async_callable(fn):
                    raise TypeError("on_observer_match: predicates and key functions are sync; they run on every call")
            self._match_hooks[function_id] = hook
            self._hook_set_by_id()
        state = self._observe_state(function_id)
        if state is not None:
            state.match_changed()

    def _set_deliver_hook(self, function_id: str, hook: DeliverHook | None) -> None:
        if hook is not None and not callable(hook):
            raise TypeError(f"on_observer_deliver: the hook must be callable, got {type(hook).__name__}")
        if hook is None:
            self._deliver_hooks.pop(function_id, None)
        else:
            self._deliver_hooks[function_id] = hook
            self._hook_set_by_id()
        if self._observe_state(function_id) is not None:
            # It decides whether the function needs observe.v2.
            self._schedule_registration(function_id)

    def observers(self, function_id: str) -> list[Observer]:
        """The active observers of a function this worker hosts, as it sees them.

        Examples:
            >>> for obs in iii.observers("cron::on-tick"):
            ...     print(obs.id, obs.args)
        """
        state = self._observe_state(function_id)
        return state.observers() if state is not None else []

    def is_observed(self, function_id: str, call: CallLike | None = None) -> bool:
        """Whether a call of a function this worker hosts would reach anyone,
        so a source can skip building an event nobody wants.

        Without ``call``: whether the function has any active observer (one
        load). With ``call`` (a :class:`Call` or a partial one,
        ``{"input": ...}``) on a function matched by keys
        (``on_observer_match``): whether an observer is filed under one of
        the call's keys. On any other function the call is ignored.

        Examples:
            >>> if iii.is_observed("database::on-row-changed", {"input": change}):
            ...     iii.call({"function_id": "database::on-row-changed", "payload": change})
        """
        state = self._observe_state(function_id)
        return state.is_observed(call) if state is not None else False

    # calling

    def call(self, request: "dict[str, Any] | CallRequest") -> Any:
        """Call a function and return its result.

        <!-- docs:expand-params -->

        The routing behavior and return type depend on the ``action`` field:

        - No action: synchronous, waits for the function to return.
        - ``CallAction.Enqueue(...)``: async via named queue, returns a dict
          with ``messageReceiptId``.
        - ``CallAction.Void()``: fire-and-forget, returns ``None``.

        A function this worker registered with a handler, once the engine
        confirmed it, runs in-process (local dispatch): a sync handler on a
        thread of its own (the caller waits up to the timeout), an async one
        on the SDK loop. Its observers are still delivered to, and only
        matched calls reach the engine. ``dispatch="remote"`` (or
        ``action=Enqueue``) always goes through the engine. Whatever a local
        handler raises, ``BaseException``s such as ``KeyboardInterrupt`` and
        ``SystemExit`` included, reaches the caller as
        ``InvocationError("invocation_failed")`` with the original as its
        ``__cause__`` (for a ``Void`` call nothing reaches the caller: the
        error is logged).

        A request with ``observer`` (an :class:`Observer` of a function this
        worker hosts) calls through it: the engine runs the observer's target
        with its policy (``max_fires``, expiry, ``max_in_flight``), metadata
        sidecar and loop guard, and the call resolves to the target's result.
        The payload goes as given: filters are applied where the worker
        chooses observers: iii's delivery, or ``obs.matches(call)`` in an
        ``on_observer_deliver``.

        Args:
            request: A ``CallRequest`` or dict with ``function_id`` (or
                ``observer``), ``payload``, and optional ``action`` /
                ``timeout_ms`` / ``metadata`` / ``namespace`` / ``dispatch``
                (``"auto"``, the default, or ``"remote"``).

        Returns:
            The function's return value for synchronous (no-action) calls,
            a ``{"messageReceiptId": ...}`` dict for enqueue actions, or
            ``None`` for void actions.

        Raises:
            InvocationError: For any engine rejection. Inspect ``code``:
                ``'TIMEOUT'`` if the invocation timed out, ``'FORBIDDEN'`` if
                RBAC denied it or the function is private to another worker;
                for a call through an observer ``'NOT_FUNCTION_OWNER'``,
                ``'OBSERVER_NOT_FOUND'``, ``'OBSERVER_NOT_ACTIVE'``,
                ``'OVERFLOW'``, ``'LOOP_GUARD'``.

        Examples:
            >>> result = worker.call({'function_id': 'greet', 'payload': {'name': 'World'}})
            >>> worker.call({'function_id': 'notify', 'payload': {}, 'action': CallAction.Void()})
            >>> worker.call({'observer': obs, 'payload': request})
        """
        req = self._prepare_call(request)
        if req.local is not None:
            return self._invoke_local_sync(req)
        return self._run_on_loop(self._invoke_remote_call(req))

    async def call_async(self, request: "dict[str, Any] | CallRequest") -> Any:
        """Call a function and return its result (on any event loop).

        The routing behavior and return type depend on the ``action`` field:

        - No action: synchronous, waits for the function to return.
        - ``CallAction.Enqueue(...)``: async via named queue, returns a dict
          with ``messageReceiptId``.
        - ``CallAction.Void()``: fire-and-forget, returns ``None``.

        A function this worker registered with a handler, once the engine
        confirmed it, runs in-process on the calling loop (a sync handler on
        a thread of its own); ``dispatch="remote"`` (or ``action=Enqueue``)
        always goes through the engine. A request with ``observer`` calls
        through it, as for :meth:`call`.

        Args:
            request: A ``CallRequest`` or dict with ``function_id`` (or
                ``observer``), ``payload``, and optional ``action`` /
                ``timeout_ms`` / ``metadata`` / ``namespace`` / ``dispatch``
                (``"auto"`` or ``"remote"``).

        Returns:
            The function's return value for synchronous (no-action) calls,
            a ``{"messageReceiptId": ...}`` dict for enqueue actions, or
            ``None`` for void actions.

        Raises:
            InvocationError: For any engine rejection. Inspect ``code``:
                ``'TIMEOUT'`` if the invocation timed out, ``'FORBIDDEN'`` if
                RBAC denied it.

        Examples:
            >>> result = await worker.call_async({'function_id': 'greet', 'payload': {'name': 'World'}})
            >>> await worker.call_async({'observer': obs, 'payload': state, 'action': CallAction.Void()})
        """
        req = self._prepare_call(request)
        if req.local is not None:
            return await self._invoke_local_async(req)
        return await self._on_sdk_loop(self._invoke_remote_call(req))

    def trigger(self, request: "dict[str, Any] | CallRequest") -> Any:
        """Call a function: :meth:`call` under its 0.25 name.

        .. deprecated:: 0.26.0
            Use :meth:`call`.
        """
        return self.call(request)

    async def trigger_async(self, request: "dict[str, Any] | CallRequest") -> Any:
        """Call a function: :meth:`call_async` under its 0.25 name.

        .. deprecated:: 0.26.0
            Use :meth:`call_async`.
        """
        return await self.call_async(request)

    def _prepare_call(self, request: "dict[str, Any] | CallRequest") -> _Request:
        """Normalizes a call request and picks the path (local or routed)."""
        if self._fatal_error is not None:
            raise self._fatal_error
        # Read field by field: `observer` is an Observer, not something to dump.
        req: dict[str, Any] = (
            request
            if isinstance(request, dict)
            else {name: getattr(request, name) for name in CallRequest.model_fields}
        )
        action = req.get("action")
        if isinstance(action, dict):
            if action.get("type") == "enqueue":
                action = CallActionEnqueue(queue=action["queue"])
            elif action.get("type") == "void":
                action = CallActionVoid()
        dispatch = req.get("dispatch")
        if dispatch not in (None, "auto", "remote"):
            raise ValueError(f"dispatch must be 'auto' or 'remote', got {dispatch!r}")
        timeout_ms = req.get("timeout_ms") or self._options.invocation_timeout_ms
        observer = req.get("observer")
        if observer is not None:
            # Through an observer: always the engine, which routes to the
            # observer's stored target (no namespace) with its policy and metadata.
            if not isinstance(observer, Observer):
                raise TypeError(f"call: observer must be an iii.Observer, got {type(observer).__name__}")
            for name in ("function_id", "namespace", "metadata"):
                if req.get(name) is not None:
                    raise ValueError(f"call: a call through an observer takes no {name}")
            if isinstance(action, CallActionEnqueue):
                raise ValueError("call: a call through an observer cannot be enqueued")
            return _Request(
                function_id=observer.target,
                payload=req.get("payload"),
                action=action,
                metadata=None,
                namespace=None,
                timeout_ms=timeout_ms,
                observer_id=observer.id,
            )
        function_id = req.get("function_id")
        if not isinstance(function_id, str) or not function_id:
            raise ValueError("call: function_id (or observer) is required")
        # Engine-owned builtins stay in ``default``. Other implicit calls stay
        # in this worker's namespace. An explicit request namespace wins.
        namespace = self._invocation_namespace(req.get("namespace"), function_id, "CallRequest.namespace")
        call = _Request(
            function_id=function_id,
            payload=req.get("payload"),
            action=action,
            metadata=req.get("metadata"),
            namespace=namespace,
            timeout_ms=timeout_ms,
        )
        # Local dispatch: a function this worker hosts, confirmed by the engine
        # for in-process calls, in the namespace it registered in. `enqueue`
        # expects a queue receipt, so it always goes to the engine.
        if dispatch != "remote" and not isinstance(action, CallActionEnqueue):
            fn = self._functions.get(function_id)
            state = fn.observe if fn is not None else None
            if (
                fn is not None
                and fn.handler is not None
                and isinstance(state, FunctionObservers)
                and state.local_dispatch
                and (namespace is None or namespace == state.namespace)
            ):
                parent = current_context()
                # Re-entrancy guard: a deeper chain of local calls goes through
                # the engine, so recursive self-calls cannot overflow the stack.
                if parent is None or parent.local_depth < _MAX_LOCAL_DEPTH:
                    call.local = fn
                    call.state = state
        return call

    async def _invoke_remote_call(self, call: _Request) -> Any:
        activation = {"observer_id": call.observer_id} if call.observer_id is not None else None
        return await self._invoke_remote(
            call.function_id,
            call.payload,
            call.action,
            call.metadata,
            call.namespace,
            call.timeout_ms,
            activation=activation,
        )

    async def _invoke_remote(
        self,
        function_id: str,
        payload: Any,
        action: Any,
        metadata: Any,
        namespace: str | None,
        timeout_ms: int | None,
        activation: dict[str, Any] | None = None,
    ) -> Any:
        """The routed path (SDK loop): an ``InvokeFunction`` through the engine."""
        if self._fatal_error is not None:
            raise self._fatal_error
        timeout_ms = timeout_ms or self._options.invocation_timeout_ms
        timeout_secs = timeout_ms / 1000.0
        caller, observe_depth = self._outgoing_observe_fields()
        observe_fields: dict[str, Any] = {
            "caller": CallerWire.model_validate(caller) if caller is not None else None,
            "observe_depth": observe_depth,
            "activation": ActivationWire.model_validate(activation) if activation is not None else None,
        }

        # Void: fire-and-forget, no response expected
        if isinstance(action, CallActionVoid):
            await self._send(
                InvokeFunctionMessage(
                    function_id=function_id,
                    data=payload,
                    metadata=metadata,
                    traceparent=self._inject_traceparent(),
                    baggage=self._inject_baggage(),
                    action=action,
                    namespace=namespace,
                    **observe_fields,
                )
            )
            return None

        # Enqueue and default: send invocation_id, await response
        invocation_id = str(uuid.uuid4())
        future: asyncio.Future[Any] = self._loop.create_future()

        self._pending[invocation_id] = _PendingInvocation(
            future=future, function_id=function_id
        )

        try:
            enqueue_action: CallActionEnqueue | None = (
                action if isinstance(action, CallActionEnqueue) else None
            )

            await self._send(
                InvokeFunctionMessage(
                    function_id=function_id,
                    data=payload,
                    metadata=metadata,
                    invocation_id=invocation_id,
                    traceparent=self._inject_traceparent(),
                    baggage=self._inject_baggage(),
                    action=enqueue_action,
                    namespace=namespace,
                    **observe_fields,
                )
            )

            try:
                return await asyncio.wait_for(future, timeout=timeout_secs)
            except asyncio.TimeoutError:
                raise InvocationError(
                    code="TIMEOUT",
                    message=f"invocation timed out after {timeout_ms}ms",
                    function_id=function_id,
                    invocation_id=invocation_id,
                )
        finally:
            # Drop the entry on every exit once it is registered: a failed or
            # cancelled send, a timeout, or the caller's await being cancelled
            # (e.g. an outer asyncio.wait_for shorter than this timeout), so a
            # late result finds nothing left to settle.
            self._pending.pop(invocation_id, None)

    # the local path

    def _local_invocation(self, call: _Request) -> _LocalInvocation:
        """The new invocation context, the values a local call works on, and
        the active set and deliver hook it starts with. A private function's
        input and metadata pass by reference, so its handler that returns its
        input reports it once (``output_is_input``). Any other function's
        handler gets copies, and its delivery a copy of the payload and the
        metadata as the caller passed them, taken here, at call start, when
        the call is delivered. The delivery sees channel refs as the caller
        passed them; the handler gets readers and writers. Raises what
        copying the payload raises."""
        state = cast(FunctionObservers, call.state)
        parent = current_context()
        caller: dict[str, Any] = dict(self._observe_host.identity())
        if parent is not None:
            caller["function_id"] = parent.function_id
            caller["invocation_id"] = parent.invocation_id
            grandparent = strip_caller(parent.caller)
            if grandparent:
                caller["parent"] = grandparent
        # The handler's metadata sidecar: its own copy, unless the function is private.
        metadata = call.metadata if state.private else _clone_value(call.metadata)
        ctx = ActiveInvocation(
            function_id=call.function_id,
            invocation_id=str(uuid.uuid4()),
            observe_depth=parent.observe_depth if parent is not None else 0,
            caller=caller,
            metadata=metadata,
            local_depth=(parent.local_depth if parent is not None else 0) + 1,
        )
        # Matched against the observers active when the call starts.
        local = _LocalInvocation(
            ctx=ctx, state=state, active=state.active, hook=self._deliver_hooks.get(call.function_id)
        )
        if state.private:
            local.call_input, local.call_metadata = call.payload, call.metadata
            local.input = self._resolve_channels(call.payload)
        else:
            if local.delivers:
                local.call_input, local.call_metadata = _clone_value(call.payload), _clone_value(call.metadata)
            local.input = self._resolve_channels(_clone_value(call.payload))
        local.metadata = metadata
        return local

    def _local_delivery(
        self, call: _Request, local: _LocalInvocation, output: Any, handler_error: BaseException | None
    ) -> Delivery | None:
        """The delivery of a finished local call, or ``None`` when nothing
        observes it and it has no deliver hook."""
        if not local.delivers:
            return None
        return Delivery(
            local.state,
            Call(
                function_id=call.function_id,
                invocation_id=local.ctx.invocation_id,
                status="success" if handler_error is None else "error",
                input=local.call_input,
                output=output,
                error=_handler_error_body(handler_error) if handler_error is not None else None,
                caller=local.ctx.caller,
                metadata=local.call_metadata,
            ),
            handler_error,
            local.active,
            routed=False,
            started_at_ms=local.started_at_ms,
            duration_ms=(time.perf_counter() - local.t0) * 1000.0,
            observe_depth=local.ctx.observe_depth,
        )

    def _count_local(self, call: _Request, local: _LocalInvocation, ok: bool) -> None:
        """Counts a finished local call (``localinvocationstats``). Never raises."""
        try:
            self._reporter.record_local(call.function_id, ok, (time.perf_counter() - local.t0) * 1000.0)
        except Exception:
            log.exception("Failed to count a local invocation of %s", call.function_id)

    @staticmethod
    def _local_error(call: _Request, error: BaseException) -> InvocationError:
        """The error a failed local call raises: the remote path's shape."""
        body = _handler_error_body(error)
        failure = InvocationError(
            code=body["code"], message=body["message"], function_id=call.function_id, stacktrace=body["stacktrace"]
        )
        failure.__cause__ = error
        return failure

    def _local_answer_error(
        self, call: _Request, error: BaseException, handler_error: BaseException | None
    ) -> InvocationError:
        """The error a local call raises: the handler's as ``invocation_failed``;
        a deliver hook's with the code of its ``ObserveRejection`` or
        ``InvocationError``."""
        if error is handler_error or not isinstance(error, (ObserveRejection, InvocationError)):
            return self._local_error(call, error)
        body = _answer_error_body(error)
        failure = InvocationError(
            code=body["code"],
            message=body["message"],
            function_id=call.function_id,
            stacktrace=body.get("stacktrace"),
        )
        failure.__cause__ = error
        return failure

    def _local_result(self, call: _Request, value: Any) -> Any:
        """What the caller gets: a copy, or the value itself from a private function."""
        return value if cast(FunctionObservers, call.state).private else _clone_value(value)

    def _local_timeout(self, call: _Request, local: _LocalInvocation) -> InvocationError:
        return InvocationError(
            code="TIMEOUT",
            message=f"invocation timed out after {call.timeout_ms}ms",
            function_id=call.function_id,
            invocation_id=local.ctx.invocation_id,
        )

    async def _run_local(self, call: _Request, local: _LocalInvocation) -> Any:
        """A local call in its invocation context: the handler, then its
        delivery, inside the ``execute`` span. Returns what the caller gets
        back, or raises the :class:`InvocationError` it gets."""
        fn = cast(RemoteFunctionData, call.local)
        handler = cast(Callable[..., Awaitable[Any]], fn.handler)
        handler_error: BaseException | None = None
        try:
            with self._execute_span(call.function_id, local.input, None, None) as outcome:
                output: Any = None
                try:
                    output = await handler(local.input, local.metadata)
                except asyncio.CancelledError:
                    raise
                except BaseException as error:
                    handler_error = error
                delivery = self._local_delivery(call, local, output, handler_error)
                if delivery is None:
                    ok, value = handler_error is None, output if handler_error is None else handler_error
                else:
                    ok, value = await delivery.run_async(local.hook)
                self._count_local(call, local, ok)
                if not ok:
                    raise guard_exit(value)
                outcome.result = value
        except _TraceContextError as error:
            cause = original_error(error, (_TraceContextError,))
            raise self._local_answer_error(call, cause, handler_error) from cause
        return value

    async def _invoke_local_async(self, call: _Request, local: _LocalInvocation | None = None) -> Any:
        """Runs the handler in-process on the calling loop (a sync handler on
        a thread of its own), with a copy of the payload, in a new invocation
        context, then its delivery; the same timeout and error shape as the
        remote path. Puts nothing on the wire when nobody observes the
        function."""
        if local is None:
            local = self._local_invocation(call)
        void = isinstance(call.action, CallActionVoid)
        token = set_context(local.ctx)
        try:
            # The task copies this context: the handler (and the `execute`
            # span) run in the new invocation.
            task = asyncio.ensure_future(self._run_local(call, local))
        finally:
            reset_context(token)

        if void:

            def logged(t: asyncio.Future[Any]) -> None:
                if not t.cancelled() and t.exception() is not None:
                    error = t.exception()
                    log.error("Error invoking function %s: %r", call.function_id, getattr(error, "__cause__", error))

            # Kept alive until it ends; its error is logged.
            self._background.add(task)
            task.add_done_callback(self._background.discard)
            task.add_done_callback(logged)
            return None

        # Raced, not cancelled: like a remote handler, it runs to its end.
        finished, _ = await asyncio.wait({task}, timeout=call.timeout_ms / 1000.0)
        if not finished:
            self._background.add(task)
            task.add_done_callback(self._background_done)
            raise self._local_timeout(call, local)
        error = task.exception()
        if error is not None:
            raise error
        return self._local_result(call, task.result())

    def _invoke_local_sync(self, call: _Request) -> Any:
        """``call()`` on the local path, from a thread: a sync handler and a
        sync deliver hook run on a thread of their own (in a copy of the
        caller's context) while the caller waits with the call's timeout, as
        the remote path does: on ``TIMEOUT`` the handler still runs to its
        end. An async handler runs on the SDK loop, and so does an async
        deliver hook."""
        if threading.current_thread() is self._thread:
            raise RuntimeError(
                "Cannot call sync SDK methods from the event loop thread. Use async handler methods instead."
            )
        fn = cast(RemoteFunctionData, call.local)
        void = isinstance(call.action, CallActionVoid)
        # The copies and snapshots are taken here, at call start, on the caller's thread.
        local = self._local_invocation(call)
        if fn.call_sync is None:
            if void:
                # Fire-and-forget: the SDK loop runs it; the context travels along.
                self._schedule_on_loop(self._invoke_local_async(call, local))
                return None
            return self._run_on_loop(self._invoke_local_async(call, local))

        sync_handler = fn.call_sync
        done: concurrent.futures.Future[Any] = concurrent.futures.Future()

        def on_sdk_loop(coro: Coroutine[Any, Any, Any]) -> Any:
            return asyncio.run_coroutine_threadsafe(coro, self._loop).result()

        def run() -> None:
            set_context(local.ctx)
            handler_error: BaseException | None = None
            try:
                with self._execute_span(call.function_id, local.input, None, None) as outcome:
                    output: Any = None
                    try:
                        output = sync_handler(local.input, local.metadata)
                    except BaseException as error:
                        # SystemExit, KeyboardInterrupt, ...: still a failed local invocation
                        handler_error = error
                    delivery = self._local_delivery(call, local, output, handler_error)
                    if delivery is None:
                        ok, value = handler_error is None, output if handler_error is None else handler_error
                    else:
                        ok, value = delivery.run_sync(local.hook, on_sdk_loop)
                    self._count_local(call, local, ok)
                    if not ok:
                        raise guard_exit(value)
                    outcome.result = value
            except _TraceContextError as error:
                cause = original_error(error, (_TraceContextError,))
                done.set_exception(self._local_answer_error(call, cause, handler_error))
                if void:
                    log.error("Error invoking function %s: %r", call.function_id, cause)
            else:
                done.set_result(value)

        # A copy of the caller's context: what the handler sets stays in it.
        threading.Thread(target=contextvars.copy_context().run, args=(run,), daemon=True).start()
        if void:
            return None
        finished, _ = concurrent.futures.wait([done], timeout=call.timeout_ms / 1000.0)
        if not finished:
            raise self._local_timeout(call, local)
        error = done.exception()
        if error is not None:
            raise error
        return self._local_result(call, done.result())

    # observe: the observing side

    def observe(
        self,
        source: "str | ObserveSource | dict[str, Any]",
        target: "Target",
        options: "ObserverOptions | dict[str, Any] | None" = None,
    ) -> ObserverHandle:
        """Observes a function: when ``source`` runs, ``target`` runs. Every
        call of the source function that passes ``source["filters"]`` invokes
        ``target``, a registered function, with the ``Observed`` envelope
        (``input``, ``output`` or ``error``, ``caller``, ...);
        ``source["deliver"]`` set to ``"input"`` or ``"output"`` gives it that
        value instead. The worker that hosts the source can decide otherwise
        in its ``on_observer_deliver``.

        Returns synchronously, also before the connection is up; the observer
        is replayed with the same id on every reconnect. Errors
        (``INVALID_ARGS``, ``INVALID_FILTER``, ``FORBIDDEN``,
        ``NOT_OBSERVABLE``, the rejection of the source's worker,
        ``UNSUPPORTED`` against an engine without observe) show up as state
        ``error`` and make ``ready()`` raise :class:`ObserveRejection`.

        Args:
            source: What is observed: a function id, or ``function_id``,
                ``namespace``, ``args`` (read by the worker that hosts the
                function, validated against its ``args_format``),
                ``filters``, and ``on`` / ``capture`` / ``deliver``.
            target: The function to invoke: its id, or
                ``{"function_id": ..., "namespace": ...}``. Register it first;
                until then the observer stays ``pending``.
            options: ``id``, ``max_fires``, ``expires_in_ms``,
                ``max_in_flight``, ``metadata`` (the activation's metadata sidecar).

        Raises:
            ValueError: ``source["function_id"]`` or ``target`` missing, or
                the id is already in use.
            TypeError: The source, target or options are not JSON.

        Examples:
            >>> iii.observe("orders::create", "alerts::big-order")
            >>> iii.register_function("alerts::payment-failed", notify)
            >>> failures = iii.observe(
            ...     {"function_id": "payments::charge", "on": "error",
            ...      "filters": {"input.amount": {"$gte": 500}}},
            ...     "alerts::payment-failed",
            ...     {"max_in_flight": 4},
            ... )
            >>> failures.ready()            # or: await failures.ready_async()
            >>> failures.dispose()
        """
        if isinstance(source, str):
            source = {"function_id": source}
        if not isinstance(source, dict):
            raise TypeError("observe: source must be a function id or a dict")
        function_id = source.get("function_id")
        if not isinstance(function_id, str) or not function_id.strip():
            raise ValueError("observe: source.function_id is required")
        target_id: Any = target.get("function_id") if isinstance(target, dict) else target
        if not isinstance(target_id, str) or not target_id.strip():
            raise ValueError("observe: target is required (the id of a registered function)")
        opts = cast("dict[str, Any]", options) if options is not None else {}
        observer_id = opts.get("id") or str(uuid.uuid4())
        if not isinstance(observer_id, str):
            raise TypeError("observe: options.id must be a string")
        if observer_id in self._observers:
            raise ValueError(f"observe: observer id already in use: {observer_id}")
        message = build_observe_message(
            observer_id,
            cast("dict[str, Any]", source),
            target,
            opts,
            lambda explicit, fid, field: self._invocation_namespace(explicit, fid, field),
        )
        self._prepare_json(message)
        handle = ObserverHandle(
            observer_id,
            message,
            self._dispose_observer,
            lambda: threading.current_thread() is self._thread,
            self._run_callback,
        )
        self._observers[observer_id] = handle
        try:
            self._loop.call_soon_threadsafe(self._after_observe, handle)
        except RuntimeError:
            pass  # shut down: it stays pending
        return handle

    def current_invocation(self) -> InvocationContext | None:
        """The invocation the calling code runs in (in every handler, remote or
        local), or ``None`` outside any handler.

        Examples:
            >>> def create_order(order):
            ...     inv = iii.current_invocation()  # function_id, invocation_id, caller, observe_depth
            ...     return {"id": inv.invocation_id if inv else None}
        """
        return public_context(current_context())

    # Internal: backing methods for items in the `iii.helpers` submodule.
    # These are renamed with a leading underscore so they don't appear on the
    # public `IIIClient` Protocol; callers use `iii.helpers.<name>(iii, ...)`.
    def _helpers_create_channel(self, buffer_size: int | None = None) -> Channel:
        """Internal shim backing :func:`iii.helpers.create_channel`.

        Public callers must use the free function from ``iii.helpers``.
        """
        return self._run_on_loop(self._helpers_create_channel_async(buffer_size))

    async def _helpers_create_channel_async(self, buffer_size: int | None = None) -> Channel:
        """Internal shim backing :func:`iii.helpers.create_channel_async`.

        Public callers must use the free function from ``iii.helpers``.
        """
        result = await self.call_async(
            {
                "function_id": "engine::channels::create",
                "payload": {"buffer_size": buffer_size},
            }
        )
        writer_ref = StreamChannelRef(**result["writer"])
        reader_ref = StreamChannelRef(**result["reader"])
        return Channel(
            writer=ChannelWriter(self._address, writer_ref, self._options.headers),
            reader=ChannelReader(self._address, reader_ref, self._options.headers),
            writer_ref=writer_ref,
            reader_ref=reader_ref,
        )

    def _get_worker_metadata(self) -> dict[str, Any]:
        try:
            sdk_version = version("iii-sdk")
        except Exception:
            sdk_version = "unknown"

        worker_name = self._worker_name()

        # One resolver, so the blank-namespace check cannot be bypassed by the
        # path that builds the metadata: a second copy of the precedence rule
        # is a second place for it to drift.
        namespace = self._worker_namespace()

        telemetry_opts = self._options.telemetry
        language = (
            (telemetry_opts.language if telemetry_opts else None)
            or os.environ.get("LANG", "").split(".")[0]
            or None
        )

        telemetry: dict[str, Any] = {
            "language": language,
            "project_name": (
                (telemetry_opts.project_name if telemetry_opts else None)
                or _detect_project_name()
            ),
            "framework": (telemetry_opts.framework if telemetry_opts else None) or "iii-py",
        }

        metadata: dict[str, Any] = {
            "runtime": "python",
            "version": sdk_version,
            "name": worker_name,
            "os": f"{platform.system()} {platform.release()} ({platform.machine()})",
            "pid": os.getpid(),
            "namespace": namespace,
            "telemetry": telemetry,
            # Opts this connection into the observe messages and host delivery.
            "features": list(SDK_FEATURES),
        }
        # Optional, like the other SDKs: only send `isolation` and `description`
        # when set so the engine's `#[serde(default)]` fields stay absent otherwise.
        isolation = os.environ.get("III_ISOLATION")
        if isolation:
            metadata["isolation"] = isolation
        if self._options.worker_description:
            metadata["description"] = self._options.worker_description
        return metadata

    def _register_worker_metadata(self) -> None:
        msg = InvokeFunctionMessage(
            function_id="engine::workers::register",
            data=self._get_worker_metadata(),
            traceparent=self._inject_traceparent(),
            baggage=self._inject_baggage(),
            action=CallActionVoid(),
        )
        asyncio.run_coroutine_threadsafe(self._send(msg), self._loop)

    def _helpers_create_stream(self, stream_name: str, stream: IStream[Any]) -> None:
        """Internal shim backing :func:`iii.helpers.create_stream`.

        Public callers must use the free function from ``iii.helpers``.
        Registers 5 of the 6 ``IStream`` methods (``get``, ``set``, ``delete``,
        ``list``, ``list_groups``). The ``update`` method is **not** registered;
        atomic updates are handled by the engine's built-in stream update
        logic.
        """

        async def get_handler(data: Any) -> Any:
            input_data = StreamGetInput(**data) if isinstance(data, dict) else data
            return await stream.get(input_data)

        async def set_handler(data: Any) -> Any:
            input_data = StreamSetInput(**data) if isinstance(data, dict) else data
            result = await stream.set(input_data)
            return result.model_dump() if result else None

        async def delete_handler(data: Any) -> Any:
            input_data = StreamDeleteInput(**data) if isinstance(data, dict) else data
            result = await stream.delete(input_data)
            return result.model_dump() if result else None

        async def list_handler(data: Any) -> list[Any]:
            input_data = StreamListInput(**data) if isinstance(data, dict) else data
            return await stream.list(input_data)

        async def list_groups_handler(data: Any) -> list[str]:
            input_data = (
                StreamListGroupsInput(**data) if isinstance(data, dict) else data
            )
            return await stream.list_groups(input_data)

        self.register_function(f"stream::get({stream_name})", get_handler)
        self.register_function(f"stream::set({stream_name})", set_handler)
        self.register_function(f"stream::delete({stream_name})", delete_handler)
        self.register_function(f"stream::list({stream_name})", list_handler)
        self.register_function(
            f"stream::list_groups({stream_name})", list_groups_handler
        )


class CallAction:
    """Factory for creating call actions used with ``call()``.

    Examples:
        >>> from iii import CallAction
        >>> # The queue must be declared in the queue worker's queue_configs.
        >>> worker.call({'function_id': 'process', 'payload': {}, 'action': CallAction.Enqueue(queue='jobs')})
        >>> worker.call({'function_id': 'notify', 'payload': {}, 'action': CallAction.Void()})
    """

    @staticmethod
    def Enqueue(*, queue: str) -> CallActionEnqueue:
        """Route the invocation through a named queue for async processing.

        Args:
            queue: Name of the target queue.
        """
        return CallActionEnqueue(queue=queue)

    @staticmethod
    def Void() -> CallActionVoid:
        """Fire-and-forget routing. No response is returned."""
        return CallActionVoid()


TriggerAction = CallAction
"""Deprecated alias of :class:`CallAction`."""


def resolve_engine_url(address: str | None = None) -> str:
    """Resolve the engine address: explicit argument, then ``III_URL``, then
    :data:`DEFAULT_ENGINE_URL`.

    The supervisor that spawned this process -- ``iii compose``, a container
    runtime, systemd -- sets ``III_URL``, the same way it sets ``III_NAMESPACE``
    and ``III_WORKER_NAME``.
    """
    if address:
        return address
    from_env = os.environ.get("III_URL")
    if from_env:
        return from_env
    return DEFAULT_ENGINE_URL


def register_worker(address: str | None = None, options: InitOptions | None = None) -> III:
    """Register the worker with a iii instance, returns a connected worker client.

    Blocks up to 30 seconds for the WebSocket connection to be established.
    If the engine is not reachable in time, a warning is logged and the
    client is returned anyway; it keeps retrying in the background and
    flushes registrations once connected. Use
    ``add_connection_state_listener`` to observe the actual transition.

    Args:
        address: WebSocket URL of the III engine (e.g. ``ws://localhost:49134``). When omitted,
            resolves from ``III_URL`` and then ``DEFAULT_ENGINE_URL``.
        options: Optional configuration for worker name, timeouts, reconnection, and OTel.

    Returns:
        An III client instance (connected, or still connecting after the
        30s wait timed out).

    Raises:
        ConnectionError: If the connection fails or exceeds max retries.

    Examples:
        >>> from iii import register_worker, InitOptions
        >>> worker = register_worker()  # address from III_URL
        >>> other = register_worker('ws://localhost:49134', InitOptions(worker_name='my-worker'))
    """
    client = III(resolve_engine_url(address), options)
    client._wait_until_connected()
    return client
