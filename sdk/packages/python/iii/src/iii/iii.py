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
import warnings
from collections.abc import Iterator
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from dataclasses import dataclass, field, replace
from importlib.metadata import version
from typing import Any, Awaitable, Callable, Coroutine, TypeVar, cast

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
from .errors import InvocationError, RegistrationRejectedError, _wrap_wire_error
from .filter._value import ABSENT, to_json_value
from .format_utils import extract_request_format, extract_response_format
from .iii_constants import (
    DEFAULT_ENGINE_URL,
    DEFAULT_RECONNECTION_CONFIG,
    MAX_QUEUE_SIZE,
    ConnectionStateCallback,
    FunctionRef,
    IIIConnectionState,
    InitOptions,
)
from .iii_types import (
    FEATURE_OBSERVE_V1,
    SDK_FEATURES,
    ActivationWire,
    CallerWire,
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
    RegisterTriggerInput,
    RegisterTriggerMessage,
    RegisterTriggerTypeInput,
    RegisterTriggerTypeMessage,
    StreamChannelRef,
    TriggerActionEnqueue,
    TriggerActionVoid,
    TriggerRequest,
    UnobserveMessage,
    UnregisterFunctionMessage,
    UnregisterTriggerMessage,
    UnregisterTriggerTypeMessage,
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
from .observable import OBSERVE_HOOK_TIMEOUT_S, FunctionObservers, Observer
from .observe_types import ObserveHook, ObserveHooks, ObserveReason, ObserverOptions, ObserveSource, Target
from .observe_wire import (
    build_observe_message,
    build_observer_ack,
    caller_for,
    decode_activation,
    decode_caller,
    decode_error,
    decode_observe_depth,
    observe_descriptor,
    strip_caller,
)
from .observer_handle import ObserverHandle, to_observer_state
from .process_marker import announce_process
from .reporter import FunctionStatus, Reporter
from .stream import IStream
from .triggers import Trigger, TriggerConfig, TriggerHandler, TriggerTypeRef
from .types import Channel, RemoteFunctionData, RemoteTriggerTypeData, is_channel_ref

RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]
TResult = TypeVar("TResult")

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

_MIGRATION_GUIDE = "https://iii.dev/docs/upgrading/from-0-24-x"

#: CONTRACT §5: one warning per process per deprecated API.
_DEPRECATIONS = {
    "register_trigger": (
        "register_trigger is deprecated and will be removed in a future release. Use "
        f"iii.observe(source, target) instead. Migration guide: {_MIGRATION_GUIDE}"
    ),
    "register_trigger_type": (
        "register_trigger_type is deprecated and will be removed in a future release. Register a function "
        "with an observe descriptor (register_function(id, handler, observe=ObserveHooks(...))) instead. "
        f"Migration guide: {_MIGRATION_GUIDE}"
    ),
}
_deprecations_warned: set[str] = set()
_deprecations_lock = threading.Lock()


def _warn_deprecated(api: str, stacklevel: int) -> None:
    """The language marker (``DeprecationWarning`` at the caller) on every
    call, and the SDK log warning once per process per API.

    ``stacklevel`` is relative to the caller of this function, as for
    ``warnings.warn``.
    """
    message = _DEPRECATIONS[api]
    warnings.warn(message, DeprecationWarning, stacklevel=stacklevel + 1)
    with _deprecations_lock:
        if api in _deprecations_warned:
            return
        _deprecations_warned.add(api)
    log.warning(message)


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


def _validate_observe_hooks(function_id: str, hooks: ObserveHooks | None) -> None:
    """``args_format`` needs ``delivery="owner"``: an auto function's events are
    its invocations, offered to every observer."""
    if hooks is None:
        return
    delivery = hooks.delivery
    if delivery is not None and delivery not in ("auto", "owner"):
        raise InvocationError(
            code="INVALID_OBSERVE_DESCRIPTOR",
            message=f"observe.delivery must be 'auto' or 'owner', got {delivery!r}",
            function_id=function_id,
        )
    if hooks.args_format is not None and delivery != "owner":
        raise InvocationError(
            code="INVALID_OBSERVE_DESCRIPTOR",
            message="observe.args_format requires delivery='owner' (an auto function takes no args)",
            function_id=function_id,
        )


def _handler_error_body(error: BaseException) -> dict[str, Any]:
    """The error a handler's failure becomes, the same on the local and the remote path."""
    return {
        "code": "invocation_failed",
        "message": str(error),
        "stacktrace": "".join(traceback.format_exception(type(error), error, error.__traceback__)),
    }


def _is_async_callable(fn: Callable[..., Any]) -> bool:
    return inspect.iscoroutinefunction(fn) or inspect.iscoroutinefunction(getattr(fn, "__call__", None))


@dataclass
class _SpanOutcome:
    result: Any = None
    traceparent: str | None = None


#: sdk.md §3: a local call nested deeper than this goes through the engine.
_MAX_LOCAL_DEPTH = 64


@dataclass
class _Call:
    """One ``trigger`` request, normalized, with the path it takes."""

    function_id: str
    payload: Any
    action: Any
    metadata: Any
    namespace: str | None
    timeout_ms: int
    #: The local function when the call runs in-process.
    local: RemoteFunctionData | None = None
    state: FunctionObservers | None = None


@dataclass
class _LocalInvocation:
    """What a local call works on: its context and the copies of the payload."""

    ctx: ActiveInvocation
    state: FunctionObservers
    compiled: tuple[Observer, ...]
    input: Any = None
    metadata: Any = None
    observed_input: Any = None
    observed_metadata: Any = None
    traceparent: str | None = None
    baggage: str | None = None
    started_at_ms: float = field(default_factory=lambda: time.time() * 1000.0)
    t0: float = field(default_factory=time.perf_counter)


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
    """What the owner side (:class:`FunctionObservers`) and the reporter need
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
        self, function_id: str, namespace: str | None, observer_id: str, revision: int, error: dict[str, str] | None
    ) -> None:
        # Sent in order with the report frames (FIFO on the SDK loop); never
        # queued while disconnected: the next functionregistered re-acks.
        client = self._client
        data = client._prepare_json(build_observer_ack(function_id, namespace, observer_id, revision, error))
        def send() -> None:
            client._send_frame(data)

        if threading.current_thread() is client._thread:
            send()
        else:
            self.call_soon(send)

    def call_target(self, target: str, observer_id: str, payload: Any, timeout_ms: int | None) -> Any:
        return self._client._run_on_loop(self._client._owner_call(target, observer_id, payload, timeout_ms))

    async def call_target_async(self, target: str, observer_id: str, payload: Any, timeout_ms: int | None) -> Any:
        return await self._client._on_sdk_loop(self._client._owner_call(target, observer_id, payload, timeout_ms))

    async def invoke_hook(self, hook: ObserveHook, obs: Observer) -> None:
        if _is_async_callable(hook):
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
        self, explicit: str | None, function_id: str, source: str = "TriggerRequest.namespace"
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
        trigger it makes now follows it there -- a whole project quietly
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
        self._triggers: dict[str, RegisterTriggerMessage] = {}
        self._trigger_types: dict[str, RemoteTriggerTypeData] = {}
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
        # contract instead of a generic ConnectionError or a hung trigger.
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
        # Re-register all (snapshot to avoid mutation from caller thread)
        for trigger_type_data in list(self._trigger_types.values()):
            await self._send(trigger_type_data.message)
        for function_data in list(self._functions.values()):
            await self._send(function_data.message)
        for trigger in list(self._triggers.values()):
            await self._send(trigger)

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
        elif msg_type == MessageType.REGISTER_TRIGGER.value:
            asyncio.create_task(self._handle_trigger_registration(data))
        elif msg_type == MessageType.UNREGISTER_TRIGGER.value:
            asyncio.create_task(self._handle_trigger_unregistration(data))
        elif msg_type == MessageType.TRIGGER_REGISTRATION_RESULT.value:
            self._handle_trigger_registration_result(data)
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

        Two codes with different semantics arrive here:

        * ``FUNCTION_NAMESPACE_CONFLICT`` -- another live worker already exports
          this one function id. Only that registration is refused; the engine
          keeps the connection open and this worker keeps serving everything
          else. Non-fatal: log and continue.
        * ``WORKER_NAMESPACE_CONFLICT`` -- another live worker already holds this
          ``(namespace, worker_name)``. The engine closes the connection. Fatal:
          stop and do NOT reconnect. Any unknown code is treated as fatal too.
        """
        code = data.get("code", "")

        if code == "FUNCTION_NAMESPACE_CONFLICT":
            # The engine refused it: no local dispatch, no observers for it here.
            state = self._observe_state(data.get("function_id", ""))
            if state is not None:
                state.on_registration_rejected(self._connection_epoch)
            log.warning(
                "Function registration rejected (%s): namespace=%r "
                "function_id=%r already owned by worker %r. Skipping that "
                "function; the worker keeps serving its other exports.",
                code,
                data.get("namespace", ""),
                data.get("function_id", ""),
                data.get("owner_worker_id", ""),
            )
            return

        error = RegistrationRejectedError(
            code=code,
            namespace=data.get("namespace", ""),
            worker_name=data.get("worker_name", ""),
            owner_worker_id=data.get("owner_worker_id", ""),
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

    def _on_engine_features(self, features: list[str]) -> None:
        """``WorkerRegistered`` told this connection what the engine supports.

        With ``observe.v1`` the held observe frames and the observers are
        sent (after the functions, replayed on connect) and held reports
        flushed; without it ``observe()`` fails with ``UNSUPPORTED``, observe
        hooks are inactive, and held observe frames are dropped.
        """
        self._engine_features = frozenset(features)
        self._registered_on_connection = True
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
        for function_id, fn in list(self._functions.items()):
            if isinstance(fn.observe, FunctionObservers) and fn.observe.needs_observe:
                self._warn_hooks_inactive(function_id)
        self._reporter.clear()

    def _warn_hooks_inactive(self, function_id: str) -> None:
        if function_id in self._hooks_inactive_warned:
            return
        self._hooks_inactive_warned.add(function_id)
        log.warning(
            "Function %r declares observe hooks or owner delivery, but the engine does not support observe "
            "(observe.v1): it is registered as a plain function and its observe hooks are inactive.",
            function_id,
        )

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

    def _send_frame(self, data: str, on_failure: Callable[[BaseException], None] | None = None) -> bool:
        """SDK loop: sends one prepared JSON frame when the socket is open.
        Its bytes count as unsent until the socket took them; ``on_failure``
        runs if the write fails."""
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
        # iii compose for the workers it starts). The engine matches live
        # registrations by name, so that identity must win over a name
        # embedded in worker code.
        return os.environ.get("III_WORKER_NAME") or self._options.worker_name or f"{platform.node()}:{os.getpid()}"

    async def _owner_call(self, target: str, observer_id: str, payload: Any, timeout_ms: int | None) -> Any:
        """``obs.call``: an ``InvokeFunction`` to the observer's target with the
        activation; no namespace (CONTRACT D13: the engine routes to the
        observer's stored target)."""
        return await self._invoke_remote(
            target, payload, None, None, None, timeout_ms, activation={"observer_id": observer_id}
        )

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

        # INTERNAL and named `execute` (not `call`/`trigger`): the engine
        # already emits the SERVER `call <fn>` span for this hop AND a
        # `trigger <fn>` span from fire_triggers. Reusing either name would
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
        if is_channel_ref(data):
            ref = StreamChannelRef(**data)
            return (
                ChannelReader(self._address, ref, self._options.headers)
                if ref.direction == "read"
                else ChannelWriter(self._address, ref, self._options.headers)
            )
        if isinstance(data, dict):
            return {k: self._resolve_channels(v) for k, v in data.items()}
        if isinstance(data, list):
            return [self._resolve_channels(v) for v in data]
        if isinstance(data, tuple):
            return tuple(self._resolve_channels(v) for v in data)
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
        # an id and are answered like any call (CONTRACT D16).
        token = set_context(
            ActiveInvocation(
                function_id=path,
                invocation_id=invocation_id or str(uuid.uuid4()),
                observe_depth=observe_depth,
                caller=caller,
                activation=activation,
                metadata=metadata,
                traceparent=traceparent,
                baggage=baggage,
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

    async def _handle_trigger_registration(self, data: dict[str, Any]) -> None:
        trigger_type_id = data.get("trigger_type")
        handler_data = (
            self._trigger_types.get(trigger_type_id) if trigger_type_id else None
        )

        trigger_id = data.get("id", "")
        function_id = data.get("function_id", "")
        config = data.get("config")
        metadata = data.get("metadata")
        namespace = data.get("namespace")

        result_base = {
            "type": MessageType.TRIGGER_REGISTRATION_RESULT.value,
            "id": trigger_id,
            "trigger_type": trigger_type_id,
            "function_id": function_id,
        }

        if not handler_data:
            return

        try:
            await handler_data.handler.register_trigger(
                TriggerConfig(
                    id=trigger_id,
                    function_id=function_id,
                    config=config,
                    metadata=metadata,
                    namespace=namespace,
                )
            )
            await self._send(result_base)
        except Exception as e:
            log.exception(f"Error registering trigger {trigger_id}")
            await self._send(
                {
                    **result_base,
                    "error": {"code": "trigger_registration_failed", "message": str(e)},
                }
            )

    def _handle_trigger_registration_result(self, data: dict[str, Any]) -> None:
        error = data.get("error")
        if not error:
            return

        trigger_id = data.get("id", "")
        trigger_type = data.get("trigger_type", "")
        message = error.get("message", "") if isinstance(error, dict) else str(error)
        log.error(
            "[iii] Trigger registration failed for %r (%s): %s",
            trigger_id,
            trigger_type,
            message,
        )

    async def _handle_trigger_unregistration(self, data: dict[str, Any]) -> None:
        trigger_type_id = data.get("trigger_type")
        if not trigger_type_id:
            return

        handler_data = self._trigger_types.get(trigger_type_id)
        if not handler_data:
            return

        trigger_id = data.get("id", "")
        function_id = data.get("function_id", "")
        config = data.get("config")
        metadata = data.get("metadata")

        try:
            await handler_data.handler.unregister_trigger(
                TriggerConfig(
                    id=trigger_id,
                    function_id=function_id,
                    config=config,
                    metadata=metadata,
                )
            )
        except Exception:
            log.exception(f"Error unregistering trigger {trigger_id}")

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
    def register_trigger_type(
        self,
        trigger_type: "RegisterTriggerTypeInput | dict[str, Any]",
        handler: TriggerHandler[Any],
    ) -> "TriggerTypeRef[Any, Any]":
        """Register a custom trigger type with the engine.

        .. deprecated:: 0.25.0
            Register a function with an observe descriptor instead:
            ``register_function(id, handler, observe=ObserveHooks(delivery="owner",
            on_observer_added=..., on_observer_removed=...))``. Emits a
            ``DeprecationWarning`` and logs a warning once per process. Migration
            guide: https://iii.dev/docs/upgrading/from-0-24-x

        Returns a :class:`TriggerTypeRef` handle with ``register_trigger``
        and ``register_function`` methods.

        Args:
            trigger_type: A ``RegisterTriggerTypeInput`` or dict with
                ``id``, ``description``, and optional ``trigger_request_format``
                / ``call_request_format`` (Pydantic class or dict).
            handler: A ``TriggerHandler`` instance.

        Returns:
            A ``TriggerTypeRef`` with typed ``register_trigger`` and
            ``register_function`` methods.

        Examples:
            >>> webhook = worker.register_trigger_type(
            ...     RegisterTriggerTypeInput(
            ...         id="webhook",
            ...         description="Webhook trigger",
            ...         trigger_request_format=WebhookConfig,
            ...         call_request_format=WebhookCallRequest,
            ...     ),
            ...     WebhookHandler(),
            ... )
            >>> webhook.register_function("handler", handle_webhook)
            >>> webhook.register_trigger("handler", WebhookConfig(url="/hook"))
        """
        _warn_deprecated("register_trigger_type", stacklevel=2)
        if isinstance(trigger_type, dict):
            trigger_type = RegisterTriggerTypeInput(**trigger_type)

        config_cls = (
            trigger_type.trigger_request_format
            if isinstance(trigger_type.trigger_request_format, type)
            else None
        )
        request_cls = (
            trigger_type.call_request_format
            if isinstance(trigger_type.call_request_format, type)
            else None
        )

        msg = RegisterTriggerTypeMessage(
            id=trigger_type.id,
            description=trigger_type.description,
            trigger_request_format=_resolve_format(trigger_type.trigger_request_format),
            call_request_format=_resolve_format(trigger_type.call_request_format),
        )
        # Isolate retained JSON fields from later caller mutations.
        msg = msg.model_copy(deep=True)
        self._send_if_connected(msg)
        self._trigger_types[trigger_type.id] = RemoteTriggerTypeData(
            message=msg, handler=handler
        )

        return TriggerTypeRef(
            iii=self,
            trigger_type_id=trigger_type.id,
            config_cls=config_cls,
            request_cls=request_cls,
        )

    def unregister_trigger_type(
        self, trigger_type: "RegisterTriggerTypeInput | dict[str, Any]"
    ) -> None:
        """Unregister a previously registered trigger type.

        .. deprecated:: 0.25.0
            Trigger types are replaced by functions with an observe descriptor;
            unregister the function instead. Migration guide:
            https://iii.dev/docs/upgrading/from-0-24-x

        Args:
            trigger_type: A ``RegisterTriggerTypeInput`` or dict with ``id``, an
                optional ``description``, and (dict only) the ``namespace`` it
                was registered in.

        Examples:
            >>> worker.unregister_trigger_type({"id": "webhook", "description": "Webhook trigger"})
            >>> worker.unregister_trigger_type(RegisterTriggerTypeInput(id="webhook", description="Webhook trigger"))
        """
        warnings.warn(
            "unregister_trigger_type is deprecated and will be removed in a future release. Trigger types are "
            f"replaced by functions with an observe descriptor. Migration guide: {_MIGRATION_GUIDE}",
            DeprecationWarning,
            stacklevel=2,
        )
        # The engine reads `{id, namespace?}` (sdk/fixtures/protocol/observe/unregistertriggertype.json).
        namespace: str | None = None
        if isinstance(trigger_type, dict):
            type_id = trigger_type["id"]
            namespace = trigger_type.get("namespace")
        else:
            type_id = trigger_type.id
        self._trigger_types.pop(type_id, None)
        self._send_if_connected(UnregisterTriggerTypeMessage(id=type_id, namespace=namespace))

    def register_trigger(
        self, trigger: RegisterTriggerInput | dict[str, Any]
    ) -> Trigger:
        """Bind a trigger configuration to a registered function.

        .. deprecated:: 0.25.0
            Use ``iii.observe(source, target)`` instead:
            ``iii.observe({"function_id": ..., "args": ..., "filters": ...}, target,
            {"metadata": ...})``. Emits a ``DeprecationWarning`` and logs a
            warning once per process. The http, cron, queue, state, stream,
            pubsub and database workers still publish trigger types: keep
            using ``register_trigger`` for them until they ship source
            functions. Migration guide:
            https://iii.dev/docs/upgrading/from-0-24-x

        <!-- docs:expand-params -->

        Args:
            trigger: A ``RegisterTriggerInput`` or dict with ``type``,
                ``function_id``, and optional ``config``.

        Returns:
            A ``Trigger`` object with an ``unregister()`` method.  The
            trigger ID is auto-generated (UUID) by the SDK and sent to
            the engine as part of the registration message.

        Examples:
            >>> trigger = worker.register_trigger({
            ...   'type': 'http',
            ...   'function_id': 'greet',
            ...   'config': {'api_path': '/greet', 'http_method': 'GET'}
            ... })
            >>> trigger = worker.register_trigger(RegisterTriggerInput(
            ...     type="http", function_id="greet",
            ...     config={'api_path': '/greet', 'http_method': 'GET'}
            ... ))
            >>> trigger.unregister()
        """
        return self._register_trigger(trigger, stacklevel=2)

    def _register_trigger(self, trigger: RegisterTriggerInput | dict[str, Any], *, stacklevel: int) -> Trigger:
        """``register_trigger``; ``stacklevel`` points the deprecation warning at the user's call."""
        _warn_deprecated("register_trigger", stacklevel=stacklevel + 1)
        if isinstance(trigger, dict):
            trigger = RegisterTriggerInput(**trigger)
        trigger_id = str(uuid.uuid4())
        msg = RegisterTriggerMessage(
            id=trigger_id,
            trigger_type=trigger.type,
            function_id=trigger.function_id,
            config=trigger.config,
            metadata=trigger.metadata,
            # Unset means this worker's namespace, not the engine's default. A
            # trigger names a function, and the function a worker registers
            # lands in the worker's namespace, so defaulting anywhere else
            # registers a trigger that fires and resolves nothing. Naming
            # another namespace, ``default`` included, stays a matter of saying
            # so.
            namespace=self._call_namespace(
                trigger.namespace, "RegisterTriggerInput.namespace"
            ),
            # Passed through untouched, ``None`` included. ``None`` is the
            # engine's two-step resolution -- this worker's namespace, then the
            # engine's own -- which carries an unmigrated worker onto the
            # engine's provider while letting a project's own win when it has
            # one. The SDK cannot decide this locally: a sibling worker in the
            # same project may be the one providing the type.
            trigger_namespace=trigger.trigger_namespace,
        )
        # Isolate retained JSON fields from later caller mutations.
        msg = msg.model_copy(deep=True)
        self._send_if_connected(msg)
        self._triggers[trigger_id] = msg

        def unregister() -> None:
            self._triggers.pop(trigger_id, None)
            self._send_if_connected(
                UnregisterTriggerMessage(id=trigger_id, trigger_type=msg.trigger_type)
            )

        return Trigger(unregister)

    def register_function(
        self,
        function_id: str,
        handler_or_invocation: RemoteFunctionHandler | HttpInvocationConfig,
        *,
        description: str | None = None,
        metadata: dict[str, Any] | None = None,
        request_format: RegisterFunctionFormat | dict[str, Any] | None = None,
        response_format: RegisterFunctionFormat | dict[str, Any] | None = None,
        observe: ObserveHooks | dict[str, Any] | None = None,
        local_dispatch: bool = True,
    ) -> FunctionRef:
        """Register a function with the engine.

        Pass a handler for local execution, or an ``HttpInvocationConfig``
        for HTTP-invoked functions (Lambda, Cloudflare Workers, etc.).

        Handlers can be synchronous or asynchronous.  Sync handlers are
        automatically wrapped with ``run_in_executor`` so they do not
        block the event loop.  Each handler receives a ``data`` argument
        containing the trigger payload, and may optionally accept a second
        ``metadata`` argument carrying per-invocation metadata (e.g.
        ``def handler(data, metadata=None)`` or ``def handler(data, *,
        metadata=None)``).  Metadata is only forwarded to handlers that
        declare a parameter literally named ``metadata``, so existing
        handlers, including ones with unrelated extra parameters,
        ``*args``, or ``**kwargs``, keep working unchanged.

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
                ``data`` (the trigger payload) as the first argument and
                may optionally accept ``metadata`` (per-invocation
                metadata) as a second argument; they may return a value.
            description: Human-readable description of what the function does.
            metadata: Arbitrary metadata attached to the function.
            request_format: Schema describing expected input.  When
                ``None`` (default), auto-extracted from the handler's
                first-parameter type hint.  Pass an explicit schema to
                override; there is no way to register with no schema
                when the handler is typed.
            response_format: Schema describing expected output.  Same
                auto-extraction semantics as ``request_format``.
            observe: Makes the function observable on its own terms: owner
                delivery (``delivery="owner"``, ``args_format``), and
                callbacks when an observer is added or removed (an
                :class:`ObserveHooks`, a subclass of it, or a dict of its
                fields). Without it the function is still observable (auto
                delivery, no args).
            local_dispatch: Calls this worker makes to its own function run
                in-process once the engine confirmed it (default ``True``).
                Like a remote invocation, each local call to a sync handler
                runs on a thread of its own (one per call, not pooled), so the
                caller can stop waiting at its timeout. A sync handler that
                blocks for long should be async, or set ``False`` so its calls
                go through the engine (also for a handler that relies on
                engine-side effects).

        Returns:
            A ``FunctionRef`` with an ``id`` attribute, an ``unregister()``
            method, and ``is_observed()`` / ``observers()`` (the active
            observers, as this owner sees them).  Call ``unregister()`` to
            remove the function from the engine.

        Raises:
            TypeError: If ``function_id`` is not a string, or if
                ``handler_or_invocation`` is not callable or
                ``HttpInvocationConfig``.
            ValueError: If ``function_id`` is empty or already registered.
            InvocationError: ``INVALID_OBSERVE_DESCRIPTOR`` when ``observe``
                declares ``args_format`` without ``delivery="owner"``, or an
                unknown ``delivery``.

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
        """
        if not isinstance(function_id, str):
            raise TypeError(
                f"function_id must be str, got {type(function_id).__name__}"
            )
        if not function_id or not function_id.strip():
            raise ValueError("id is required")
        if function_id in self._functions:
            raise ValueError(f"function id '{function_id}' already registered")

        # The hooks stay here; only the descriptor's four fields travel, and
        # `local_dispatch` is this SDK's alone.
        hooks = ObserveHooks.from_value(observe)
        _validate_observe_hooks(function_id, hooks)
        descriptor = observe_descriptor(hooks, _resolve_format(hooks.args_format) if hooks is not None else None)

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

        if isinstance(handler_or_invocation, HttpInvocationConfig):
            msg = RegisterFunctionMessage(
                id=func.id,
                invocation=handler_or_invocation,
                description=func.description,
                metadata=func.metadata,
                request_format=func.request_format,
                response_format=func.response_format,
                observe=descriptor,
            )
            # Isolate retained JSON fields from later caller mutations.
            msg = msg.model_copy(deep=True)
            observe_state = FunctionObservers(function_id, hooks, local_dispatch, False, self._observe_host)
            data = RemoteFunctionData(message=msg, observe=observe_state)
            self._prepare_json(msg)
            # Stored before it is sent: the engine's `functionregistered` must find it.
            self._functions[func.id] = data
            self._send_if_connected(msg)
        else:
            if not callable(handler_or_invocation):
                actual_type = type(handler_or_invocation).__name__
                raise TypeError(
                    f"handler_or_invocation must be callable or HttpInvocationConfig, got {actual_type}"
                )
            handler = cast("Callable[..., Any]", handler_or_invocation)
            msg = RegisterFunctionMessage(
                id=func.id,
                description=func.description,
                metadata=func.metadata,
                request_format=func.request_format,
                response_format=func.response_format,
                observe=descriptor,
            )
            # Isolate retained JSON fields from later caller mutations.
            msg = msg.model_copy(deep=True)
            self._prepare_json(msg)

            # Decide once, at registration time, whether this handler accepts
            # per-invocation metadata. 1-arg handlers keep working unchanged.
            metadata_mode = _metadata_passing_mode(handler)

            def _call_handler(input_data: Any, metadata: dict[str, Any] | None) -> Any:
                if metadata_mode == "positional":
                    return handler(input_data, metadata)
                if metadata_mode == "keyword":
                    return handler(input_data, metadata=metadata)
                return handler(input_data)

            is_async = _is_async_callable(handler)
            if is_async:

                async def wrapped(input_data: Any, metadata: dict[str, Any] | None = None) -> Any:
                    return await _call_handler(input_data, metadata)

            else:

                async def wrapped(input_data: Any, metadata: dict[str, Any] | None = None) -> Any:
                    # A thread of its own, started in a copy of this context:
                    # the current invocation and the OpenTelemetry context
                    # (the `execute` span) follow the handler.
                    return await run_in_thread(_call_handler, input_data, metadata)

            observe_state = FunctionObservers(function_id, hooks, local_dispatch, True, self._observe_host)
            data = RemoteFunctionData(
                message=msg,
                handler=wrapped,
                observe=observe_state,
                call_sync=None if is_async else _call_handler,
            )
            # Stored before it is sent: the engine's `functionregistered` must find it.
            self._functions[func.id] = data
            self._send_if_connected(msg)

        if (
            observe_state.needs_observe
            and self._registered_on_connection
            and FEATURE_OBSERVE_V1 not in self._engine_features
        ):
            self._warn_hooks_inactive(function_id)

        func_id = func.id

        def unregister() -> None:
            if self._functions.get(func_id) is data:
                del self._functions[func_id]
            self._send_if_connected(UnregisterFunctionMessage(id=func_id))
            observe_state.dispose()

        return FunctionRef(
            id=func_id,
            unregister=unregister,
            _is_observed=observe_state.is_observed,
            _observers=observe_state.observers,
        )

    def trigger(self, request: "dict[str, Any] | TriggerRequest") -> Any:
        """Invoke a remote function.

        <!-- docs:expand-params -->

        The routing behavior and return type depend on the ``action`` field:

        - No action: synchronous, waits for the function to return.
        - ``TriggerAction.Enqueue(...)``: async via named queue, returns a dict
          with ``messageReceiptId``.
        - ``TriggerAction.Void()``: fire-and-forget, returns ``None``.

        A function this worker registered with a handler, once the engine
        confirmed it, runs in-process (local dispatch, sdk.md §3): a sync
        handler on a thread of its own (the caller waits up to the timeout),
        an async one on the SDK loop. Its observers are still evaluated, and
        only matched calls reach the engine. ``dispatch="remote"`` (or
        ``action=Enqueue``) always goes through the engine. Whatever a local
        handler raises, ``BaseException``s such as ``KeyboardInterrupt`` and
        ``SystemExit`` included, reaches the caller as
        ``InvocationError("invocation_failed")`` with the original as its
        ``__cause__`` (for a ``Void`` call nothing reaches the caller: the
        error is logged).

        Args:
            request: A ``TriggerRequest`` or dict with ``function_id``,
                ``payload``, and optional ``action`` / ``timeout_ms`` /
                ``metadata`` / ``namespace`` / ``dispatch`` (``"auto"``, the
                default, or ``"remote"``).

        Returns:
            The function's return value for synchronous (no-action) calls,
            a ``{"messageReceiptId": ...}`` dict for enqueue actions, or
            ``None`` for void actions.

        Raises:
            InvocationError: For any engine rejection. Inspect ``code``:
                ``'TIMEOUT'`` if the invocation timed out, ``'FORBIDDEN'`` if
                RBAC denied it.

        Examples:
            >>> result = worker.trigger({'function_id': 'greet', 'payload': {'name': 'World'}})
            >>> worker.trigger({'function_id': 'notify', 'payload': {}, 'action': TriggerAction.Void()})
        """
        call = self._prepare_call(request)
        if call.local is not None:
            return self._invoke_local_sync(call)
        return self._run_on_loop(self._invoke_remote_call(call))

    async def trigger_async(self, request: "dict[str, Any] | TriggerRequest") -> Any:
        """Invoke a remote function.

        The routing behavior and return type depend on the ``action`` field:

        - No action: synchronous, waits for the function to return.
        - ``TriggerAction.Enqueue(...)``: async via named queue, returns a dict
          with ``messageReceiptId``.
        - ``TriggerAction.Void()``: fire-and-forget, returns ``None``.

        Works from any event loop. A function this worker registered with a
        handler, once the engine confirmed it, runs in-process on the
        calling loop (a sync handler on a thread of its own);
        ``dispatch="remote"`` (or ``action=Enqueue``) always goes through
        the engine.

        Args:
            request: A ``TriggerRequest`` or dict with ``function_id``, ``payload``,
                and optional ``action`` / ``timeout_ms`` / ``metadata`` /
                ``namespace`` / ``dispatch`` (``"auto"`` or ``"remote"``).

        Returns:
            The function's return value for synchronous (no-action) calls,
            a ``{"messageReceiptId": ...}`` dict for enqueue actions, or
            ``None`` for void actions.

        Raises:
            InvocationError: For any engine rejection. Inspect ``code``:
                ``'TIMEOUT'`` if the invocation timed out, ``'FORBIDDEN'`` if
                RBAC denied it.

        Examples:
            >>> result = await worker.trigger_async({'function_id': 'greet', 'payload': {'name': 'World'}})
            >>> await worker.trigger_async({'function_id': 'notify', 'payload': {}, 'action': TriggerAction.Void()})
        """
        call = self._prepare_call(request)
        if call.local is not None:
            return await self._invoke_local_async(call)
        return await self._on_sdk_loop(self._invoke_remote_call(call))

    def _prepare_call(self, request: "dict[str, Any] | TriggerRequest") -> "_Call":
        """Normalizes a trigger request and picks the path (sdk.md §3)."""
        if self._fatal_error is not None:
            raise self._fatal_error
        req = request if isinstance(request, dict) else request.model_dump()
        function_id = req["function_id"]
        action = req.get("action")
        if isinstance(action, dict):
            if action.get("type") == "enqueue":
                action = TriggerActionEnqueue(queue=action["queue"])
            elif action.get("type") == "void":
                action = TriggerActionVoid()
        dispatch = req.get("dispatch")
        if dispatch not in (None, "auto", "remote"):
            raise ValueError(f"dispatch must be 'auto' or 'remote', got {dispatch!r}")
        # Engine-owned builtins stay in ``default``. Other implicit calls stay
        # in this worker's namespace. An explicit request namespace wins.
        namespace = self._invocation_namespace(req.get("namespace"), function_id)
        call = _Call(
            function_id=function_id,
            payload=req.get("payload"),
            action=action,
            metadata=req.get("metadata"),
            namespace=namespace,
            timeout_ms=req.get("timeout_ms") or self._options.invocation_timeout_ms,
        )
        # Local dispatch: a function this worker hosts, confirmed by the engine
        # for in-process calls, in the namespace it registered in. `enqueue`
        # expects a queue receipt, so it always goes to the engine.
        if dispatch != "remote" and not isinstance(action, TriggerActionEnqueue):
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

    async def _invoke_remote_call(self, call: "_Call") -> Any:
        return await self._invoke_remote(
            call.function_id, call.payload, call.action, call.metadata, call.namespace, call.timeout_ms
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
        if isinstance(action, TriggerActionVoid):
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
            enqueue_action: TriggerActionEnqueue | None = (
                action if isinstance(action, TriggerActionEnqueue) else None
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

    # observe: the local path (sdk.md §4.3)

    def _local_invocation(self, call: "_Call") -> "_LocalInvocation":
        """The new invocation context and the copies a local call works on.
        Raises what copying the payload raises."""
        state = cast(FunctionObservers, call.state)
        parent = current_context()
        caller: dict[str, Any] = dict(self._observe_host.identity())
        if parent is not None:
            caller["function_id"] = parent.function_id
            caller["invocation_id"] = parent.invocation_id
            grandparent = strip_caller(parent.caller)
            if grandparent:
                caller["parent"] = grandparent
        # The handler's own copy of the metadata sidecar: what it receives, and
        # what the events it emits see as `metadata`.
        metadata = _clone_value(call.metadata)
        ctx = ActiveInvocation(
            function_id=call.function_id,
            invocation_id=str(uuid.uuid4()),
            observe_depth=parent.observe_depth if parent is not None else 0,
            caller=caller,
            metadata=metadata,
            local_depth=(parent.local_depth if parent is not None else 0) + 1,
        )
        # Evaluated against the observers active when the call starts; nothing
        # is copied for reporting when there are none.
        compiled = state.compiled if state.delivery == "auto" else ()
        local = _LocalInvocation(ctx=ctx, state=state, compiled=compiled)
        if compiled:
            # What observers see is what the caller passed at the start (input
            # and metadata), not what anyone made of them by the end.
            local.observed_input = _clone_value(call.payload)
            local.observed_metadata = _clone_value(call.metadata)
            local.traceparent, local.baggage = self._inject_traceparent(), self._inject_baggage()
        local.input = self._resolve_channels(_clone_value(call.payload))
        local.metadata = metadata
        return local

    def _finish_local(
        self, call: "_Call", local: "_LocalInvocation", ok: bool, output: Any, error: BaseException | None
    ) -> None:
        """Counts a finished local call and reports it when an observer matches. Never raises."""
        try:
            duration_ms = (time.perf_counter() - local.t0) * 1000.0
            self._reporter.record_local(call.function_id, ok, duration_ms)
            if local.compiled:
                local.state.report_local(
                    local.compiled,
                    invocation_id=local.ctx.invocation_id,
                    ok=ok,
                    input_=local.observed_input,
                    output=output,
                    error=_handler_error_body(error) if error is not None else None,
                    caller=local.ctx.caller,
                    metadata=local.observed_metadata,
                    started_at_ms=local.started_at_ms,
                    duration_ms=duration_ms,
                    observe_depth=local.ctx.observe_depth,
                    traceparent=local.traceparent,
                    baggage=local.baggage,
                )
        except Exception:
            log.exception("Failed to report a local invocation of %s", call.function_id)

    @staticmethod
    def _local_error(call: "_Call", error: BaseException) -> InvocationError:
        """The error a failed local call raises: the remote path's shape."""
        body = _handler_error_body(error)
        failure = InvocationError(
            code=body["code"], message=body["message"], function_id=call.function_id, stacktrace=body["stacktrace"]
        )
        failure.__cause__ = error
        return failure

    def _local_timeout(self, call: "_Call", local: "_LocalInvocation") -> InvocationError:
        return InvocationError(
            code="TIMEOUT",
            message=f"invocation timed out after {call.timeout_ms}ms",
            function_id=call.function_id,
            invocation_id=local.ctx.invocation_id,
        )

    async def _invoke_local_async(self, call: "_Call", local: "_LocalInvocation | None" = None) -> Any:
        """Runs the handler in-process on the calling loop (a sync handler on
        a thread of its own), with a copy of the payload, in a new invocation
        context; the same timeout and error shape as the remote path. Puts
        nothing on the wire when nobody observes the function."""
        fn = cast(RemoteFunctionData, call.local)
        handler = cast(Callable[..., Awaitable[Any]], fn.handler)
        if local is None:
            local = self._local_invocation(call)
        started = local
        void = isinstance(call.action, TriggerActionVoid)
        token = set_context(local.ctx)
        try:
            # The task copies this context: the handler (and the `execute`
            # span) run in the new invocation.
            task = asyncio.ensure_future(
                self._invoke_with_otel_context(call.function_id, handler, local.input, None, None, local.metadata)
            )
        finally:
            reset_context(token)

        def done(t: "asyncio.Future[tuple[Any, str | None]]") -> None:
            if t.cancelled():
                return
            error = t.exception()
            if error is None:
                self._finish_local(call, started, True, t.result()[0], None)
                return
            original = original_error(error, (_TraceContextError,))
            self._finish_local(call, started, False, None, original)
            if void:
                log.error("Error invoking function %s: %r", call.function_id, original)

        task.add_done_callback(done)

        if void:
            # Kept alive until it ends; its error is logged above.
            self._background.add(task)
            task.add_done_callback(self._background.discard)
            return None

        # Raced, not cancelled: like a remote handler, it runs to its end.
        finished, _ = await asyncio.wait({task}, timeout=call.timeout_ms / 1000.0)
        if not finished:
            self._background.add(task)
            task.add_done_callback(self._background_done)
            raise self._local_timeout(call, local)
        error = task.exception()
        if error is not None:
            raise self._local_error(call, original_error(error, (_TraceContextError,)))
        return _clone_value(task.result()[0])

    def _invoke_local_sync(self, call: "_Call") -> Any:
        """``trigger()`` on the local path, from a thread: a sync handler runs
        on a thread of its own (in a copy of the caller's context) while the
        caller waits with the call's timeout, as the remote path does: on
        ``TIMEOUT`` the handler still runs to its end. An async handler runs
        on the SDK loop."""
        if threading.current_thread() is self._thread:
            raise RuntimeError(
                "Cannot call sync SDK methods from the event loop thread. Use async handler methods instead."
            )
        fn = cast(RemoteFunctionData, call.local)
        void = isinstance(call.action, TriggerActionVoid)
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

        def run() -> None:
            set_context(local.ctx)
            try:
                with self._execute_span(call.function_id, local.input, None, None) as outcome:
                    outcome.result = sync_handler(local.input, local.metadata)
            except _TraceContextError as error:
                cause = original_error(error, (_TraceContextError,))
                self._finish_local(call, local, False, None, cause)
                done.set_exception(cause)
                if void:
                    log.error("Error invoking function %s: %s", call.function_id, cause)
            except BaseException as error:
                # SystemExit, KeyboardInterrupt, ...: still a failed local invocation
                self._finish_local(call, local, False, None, error)
                done.set_exception(error)
                if void:
                    log.error("Error invoking function %s: %r", call.function_id, error)
            else:
                self._finish_local(call, local, True, outcome.result, None)
                done.set_result(outcome.result)

        # A copy of the caller's context: what the handler sets stays in it.
        threading.Thread(target=contextvars.copy_context().run, args=(run,), daemon=True).start()
        if void:
            return None
        finished, _ = concurrent.futures.wait([done], timeout=call.timeout_ms / 1000.0)
        if not finished:
            raise self._local_timeout(call, local)
        error = done.exception()
        if error is not None:
            raise self._local_error(call, error) from error
        return _clone_value(done.result())

    # observe: the observing side

    def observe(
        self,
        source: "ObserveSource | dict[str, Any]",
        target: "Target",
        options: "ObserverOptions | dict[str, Any] | None" = None,
    ) -> ObserverHandle:
        """Observes a function: every event of ``source`` (an invocation of an
        auto function, or what its owner hands over) that passes
        ``source["filters"]`` invokes ``target``, a registered function.

        Returns synchronously, also before the connection is up; the observer
        is replayed with the same id on every reconnect. Errors
        (``INVALID_ARGS``, ``INVALID_FILTER``, ``FORBIDDEN``, an owner's
        rejection, ``UNSUPPORTED`` against an engine without observe) show up
        as state ``error`` and make ``ready()`` raise :class:`ObserveRejection`.

        Args:
            source: What is observed: ``function_id``, ``namespace``,
                ``args`` (owner-delivered functions only), ``filters``, and
                ``on`` / ``capture`` / ``deliver``.
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
        if not isinstance(source, dict):
            raise TypeError("observe: source must be a dict")
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
        result = await self.trigger_async(
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
            "isolation": os.environ.get("III_ISOLATION") or None,
            "namespace": namespace,
            "telemetry": telemetry,
            # Opts this connection into the observe messages (CONTRACT D2).
            "features": list(SDK_FEATURES),
        }
        # Optional, like the other SDKs: only send `description` when set so the
        # engine's `#[serde(default)]` field stays absent otherwise.
        if self._options.worker_description:
            metadata["description"] = self._options.worker_description
        return metadata

    def _register_worker_metadata(self) -> None:
        msg = InvokeFunctionMessage(
            function_id="engine::workers::register",
            data=self._get_worker_metadata(),
            traceparent=self._inject_traceparent(),
            baggage=self._inject_baggage(),
            action=TriggerActionVoid(),
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


class TriggerAction:
    """Factory for creating trigger actions used with ``trigger()``.

    Examples:
        >>> from iii import TriggerAction
        >>> # The queue must be declared in the queue worker's queue_configs.
        >>> worker.trigger({'function_id': 'process', 'payload': {}, 'action': TriggerAction.Enqueue(queue='jobs')})
        >>> worker.trigger({'function_id': 'notify', 'payload': {}, 'action': TriggerAction.Void()})
    """

    @staticmethod
    def Enqueue(*, queue: str) -> TriggerActionEnqueue:
        """Route the invocation through a named queue for async processing.

        Args:
            queue: Name of the target queue.
        """
        return TriggerActionEnqueue(queue=queue)

    @staticmethod
    def Void() -> TriggerActionVoid:
        """Fire-and-forget routing. No response is returned."""
        return TriggerActionVoid()


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
