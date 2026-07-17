"""III SDK implementation for WebSocket communication with the III Engine."""

import asyncio
import inspect
import json
import logging
import os
import platform
import random
import threading
import traceback
import uuid
from dataclasses import dataclass
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

from .channels import ChannelReader, ChannelWriter
from .errors import InvocationError, RegistrationRejectedError, _wrap_wire_error
from .format_utils import extract_request_format, extract_response_format
from .iii_constants import (
    DEFAULT_RECONNECTION_CONFIG,
    MAX_QUEUE_SIZE,
    ConnectionStateCallback,
    FunctionRef,
    IIIConnectionState,
    InitOptions,
)
from .iii_types import (
    InvocationResultMessage,
    InvokeFunctionMessage,
    MessageType,
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
    UnregisterFunctionMessage,
    UnregisterTriggerMessage,
    UnregisterTriggerTypeMessage,
)
from .stream import IStream
from .triggers import Trigger, TriggerConfig, TriggerHandler, TriggerTypeRef
from .types import Channel, RemoteFunctionData, RemoteTriggerTypeData, is_channel_ref

RemoteFunctionHandler = Callable[[Any], Awaitable[Any]]
TResult = TypeVar("TResult")

log = logging.getLogger("iii.iii")


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


class III:
    """WebSocket client for communication with the III Engine.

    Use ``register_worker(address, options)`` as the primary entry point.
    It creates the client and blocks until the connection is established.

    Args:
        address: WebSocket URL of the III engine (e.g. ``ws://localhost:49134``).
        options: Optional configuration. See ``InitOptions``.

    Examples:
        >>> from iii import register_worker, InitOptions
        >>> worker = register_worker('ws://localhost:49134', InitOptions(worker_name='my-worker'))
    """

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

        # Background event loop thread
        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._loop.run_forever, daemon=False)
        self._thread.start()

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
        if self._connection_state == "connected":
            return
        if self._connection_state == "failed":
            raise ConnectionError(f"Connection to {self._address} failed")
        connected = self._connected_event.wait(timeout=30)
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
        otel_cfg: OtelConfig | None = None
        if self._options.otel:
            if isinstance(self._options.otel, OtelConfig):
                otel_cfg = self._options.otel
            else:
                otel_cfg = OtelConfig(**self._options.otel)
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

        for task in [self._reconnect_task, self._receiver_task]:
            if task:
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass

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
        try:
            log.debug(f"Connecting to {self._address}")
            self._ws = await websockets.connect(
                self._address,
                additional_headers=self._options.headers,
            )
            log.info(f"Connected to {self._address}")
            await self._on_connected()
        except Exception as e:
            # Catch everything: a connect-time failure must never kill the
            # reconnect loop. Enumerating exception types here has failed
            # twice — ConnectionError/OSError missed websockets'
            # InvalidHandshake (MOT-3931), and that in turn missed
            # InvalidMessage, raised when the engine-boot listener race
            # surfaces through iii-network's accept-then-dial as an EOF
            # between TCP connect and the WS 101 (MOT-3966): the exception
            # escaped, the connect task died silently, and the worker stayed
            # a zombie with zero retries. CancelledError is BaseException,
            # so shutdown cancellation still propagates.
            log.warning(f"Connection failed: {type(e).__name__}: {e}")
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
        self._reconnect_attempt = 0
        self._set_connection_state("connected")
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
            if self._ws:
                await self._ws.send(json.dumps(queued_msg))

        # Register worker metadata
        self._register_worker_metadata()

        self._receiver_task = asyncio.create_task(self._receive_loop())

    async def _receive_loop(self) -> None:
        if not self._ws:
            return
        try:
            async for msg in self._ws:
                await self._handle_message(msg)
        except websockets.ConnectionClosed:
            log.debug("Connection closed")
            self._ws = None
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
            return data
        return {"data": msg}

    async def _send(self, msg: Any) -> None:
        data = self._to_dict(msg)
        if self._ws and self._ws.state.name == "OPEN":
            log.debug(f"Send: {json.dumps(data)[:200]}")
            await self._ws.send(json.dumps(data))
        else:
            if len(self._queue) >= MAX_QUEUE_SIZE:
                log.warning("Message queue full, dropping oldest message")
                self._queue.pop(0)
            self._queue.append(data)

    def _enqueue(self, msg: Any) -> None:
        data = self._to_dict(msg)
        if len(self._queue) >= MAX_QUEUE_SIZE:
            log.warning("Message queue full, dropping oldest message")
            self._queue.pop(0)
        self._queue.append(data)

    def _send_if_connected(self, msg: Any) -> None:
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
                )
            )
        elif msg_type == MessageType.REGISTER_TRIGGER.value:
            asyncio.create_task(self._handle_trigger_registration(data))
        elif msg_type == MessageType.UNREGISTER_TRIGGER.value:
            asyncio.create_task(self._handle_trigger_unregistration(data))
        elif msg_type == MessageType.TRIGGER_REGISTRATION_RESULT.value:
            self._handle_trigger_registration_result(data)
        elif msg_type == MessageType.WORKER_REGISTERED.value:
            worker_id = data.get("worker_id", "")
            self._worker_id = worker_id
            log.debug(f"Worker registered with ID: {worker_id}")
        elif msg_type == MessageType.REGISTRATION_REJECTED.value:
            self._handle_registration_rejected(data)

    def _handle_registration_rejected(self, data: dict[str, Any]) -> None:
        """Handle a ``registrationrejected`` push from the engine.

        Two codes with different semantics arrive here:

        * ``FUNCTION_NAMESPACE_CONFLICT`` -- another live worker already exports
          this one function id. Only that registration is refused; the engine
          keeps the connection open and this worker keeps serving everything
          else. Non-fatal: log and continue. (Here ``worker_name`` carries the
          conflicting function id -- the engine reuses the struct.)
        * ``WORKER_NAMESPACE_CONFLICT`` -- another live worker already holds this
          ``(namespace, worker_name)``. The engine closes the connection. Fatal:
          stop and do NOT reconnect. Any unknown code is treated as fatal too.
        """
        code = data.get("code", "")

        if code == "FUNCTION_NAMESPACE_CONFLICT":
            log.warning(
                "Function registration rejected (%s): namespace=%r "
                "function_id=%r already owned by worker %r. Skipping that "
                "function; the worker keeps serving its other exports.",
                code,
                data.get("namespace", ""),
                data.get("worker_name", ""),
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

    def _handle_result(self, invocation_id: str, result: Any, error: Any) -> None:
        pending = self._pending.pop(invocation_id, None)
        if not pending:
            log.debug(f"No pending invocation: {invocation_id}")
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

    async def _invoke_with_otel_context(
        self,
        function_id: str,
        handler: Callable[..., Awaitable[Any]],
        data: Any,
        traceparent: str | None,
        baggage: str | None,
        metadata: dict[str, Any] | None = None,
    ) -> tuple[Any, str | None]:
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
        import os

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
            try:
                result = await handler(data, metadata)
                if trace_payloads and span.is_recording():
                    out_json, out_truncated = redact_and_truncate(result, payload_max_bytes)
                    span.add_event(
                        "iii.invocation.output",
                        attributes={
                            "iii.payload.json": out_json,
                            "iii.payload.truncated": out_truncated,
                            "iii.payload.ok": True,
                        },
                    )
                span.set_status(trace.StatusCode.OK)
                response_traceparent = self._inject_traceparent()
                return result, response_traceparent
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

    def _resolve_channels(self, data: Any) -> Any:
        if is_channel_ref(data):
            ref = StreamChannelRef(**data)
            return (
                ChannelReader(self._address, ref)
                if ref.direction == "read"
                else ChannelWriter(self._address, ref)
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
                TriggerConfig(id=trigger_id, function_id=function_id, config=config, metadata=metadata)
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
        message = error.get("message", "")
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
        Treat calls as state notifications, not edges — a state may rarely
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
        self._trigger_types[trigger_type.id] = RemoteTriggerTypeData(
            message=msg, handler=handler
        )
        self._send_if_connected(msg)

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

        Args:
            trigger_type: A ``RegisterTriggerTypeInput`` or dict with ``id`` and optional ``description``.

        Examples:
            >>> worker.unregister_trigger_type({"id": "webhook", "description": "Webhook trigger"})
            >>> worker.unregister_trigger_type(RegisterTriggerTypeInput(id="webhook", description="Webhook trigger"))
        """
        if isinstance(trigger_type, dict):
            type_id = trigger_type["id"]
        else:
            type_id = trigger_type.id
        self._trigger_types.pop(type_id, None)
        self._send_if_connected(UnregisterTriggerTypeMessage(id=type_id))

    def register_trigger(
        self, trigger: RegisterTriggerInput | dict[str, Any]
    ) -> Trigger:
        """Bind a trigger configuration to a registered function.

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
        if isinstance(trigger, dict):
            trigger = RegisterTriggerInput(**trigger)
        trigger_id = str(uuid.uuid4())
        msg = RegisterTriggerMessage(
            id=trigger_id,
            trigger_type=trigger.type,
            function_id=trigger.function_id,
            config=trigger.config,
            metadata=trigger.metadata,
        )
        self._triggers[trigger_id] = msg
        self._send_if_connected(msg)

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

        Returns:
            A ``FunctionRef`` with an ``id`` attribute and an
            ``unregister()`` method.  Call ``unregister()`` to remove
            the function from the engine.

        Raises:
            TypeError: If ``function_id`` is not a string, or if
                ``handler_or_invocation`` is not callable or
                ``HttpInvocationConfig``.
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
        """
        if not isinstance(function_id, str):
            raise TypeError(
                f"function_id must be str, got {type(function_id).__name__}"
            )
        if not function_id or not function_id.strip():
            raise ValueError("id is required")
        if function_id in self._functions:
            raise ValueError(f"function id '{function_id}' already registered")

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
            )
            self._send_if_connected(msg)
            self._functions[func.id] = RemoteFunctionData(message=msg)
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
            )
            self._send_if_connected(msg)

            # Decide once, at registration time, whether this handler accepts
            # per-invocation metadata. 1-arg handlers keep working unchanged.
            metadata_mode = _metadata_passing_mode(handler)

            def _call_handler(input_data: Any, metadata: dict[str, Any] | None) -> Any:
                if metadata_mode == "positional":
                    return handler(input_data, metadata)
                if metadata_mode == "keyword":
                    return handler(input_data, metadata=metadata)
                return handler(input_data)

            if asyncio.iscoroutinefunction(handler):

                async def wrapped(input_data: Any, metadata: dict[str, Any] | None = None) -> Any:
                    return await _call_handler(input_data, metadata)

            else:

                async def wrapped(input_data: Any, metadata: dict[str, Any] | None = None) -> Any:
                    loop = asyncio.get_running_loop()
                    future: asyncio.Future[Any] = loop.create_future()

                    def _run() -> None:
                        try:
                            result = _call_handler(input_data, metadata)
                            loop.call_soon_threadsafe(future.set_result, result)
                        except BaseException as exc:
                            loop.call_soon_threadsafe(future.set_exception, exc)

                    t = threading.Thread(target=_run, daemon=True)
                    t.start()
                    return await future

            self._functions[func.id] = RemoteFunctionData(message=msg, handler=wrapped)

        func_id = func.id

        def unregister() -> None:
            self._functions.pop(func_id, None)
            self._send_if_connected(UnregisterFunctionMessage(id=func_id))

        return FunctionRef(id=func_id, unregister=unregister)

    def trigger(self, request: "dict[str, Any] | TriggerRequest") -> Any:
        """Invoke a remote function.

        <!-- docs:expand-params -->

        The routing behavior and return type depend on the ``action`` field:

        - No action: synchronous, waits for the function to return.
        - ``TriggerAction.Enqueue(...)``: async via named queue, returns a dict
          with ``messageReceiptId``.
        - ``TriggerAction.Void()``: fire-and-forget, returns ``None``.

        Args:
            request: A ``TriggerRequest`` or dict with ``function_id``,
                ``payload``, and optional ``action`` / ``timeout_ms``.

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
        return self._run_on_loop(self.trigger_async(request))

    async def trigger_async(self, request: "dict[str, Any] | TriggerRequest") -> Any:
        """Invoke a remote function.

        The routing behavior and return type depend on the ``action`` field:

        - No action: synchronous, waits for the function to return.
        - ``TriggerAction.Enqueue(...)``: async via named queue, returns a dict
          with ``messageReceiptId``.
        - ``TriggerAction.Void()``: fire-and-forget, returns ``None``.

        Args:
            request: A ``TriggerRequest`` or dict with ``function_id``, ``payload``,
                and optional ``action`` / ``timeout_ms``.

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
        req = request if isinstance(request, dict) else request.model_dump()
        function_id = req["function_id"]
        payload = req.get("payload")
        action = req.get("action")
        metadata = req.get("metadata")
        namespace = req.get("namespace")

        timeout_ms = req.get("timeout_ms") or self._options.invocation_timeout_ms

        timeout_secs = timeout_ms / 1000.0

        if isinstance(action, dict):
            if action.get("type") == "enqueue":
                action = TriggerActionEnqueue(queue=action["queue"])
            elif action.get("type") == "void":
                action = TriggerActionVoid()

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
                )
            )
            return None

        # Enqueue and default: send invocation_id, await response
        invocation_id = str(uuid.uuid4())
        future: asyncio.Future[Any] = self._loop.create_future()

        self._pending[invocation_id] = _PendingInvocation(
            future=future, function_id=function_id
        )

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
            )
        )

        try:
            return await asyncio.wait_for(future, timeout=timeout_secs)
        except asyncio.TimeoutError:
            self._pending.pop(invocation_id, None)
            raise InvocationError(
                code="TIMEOUT",
                message=f"invocation timed out after {timeout_ms}ms",
                function_id=function_id,
                invocation_id=invocation_id,
            )

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
            writer=ChannelWriter(self._address, writer_ref),
            reader=ChannelReader(self._address, reader_ref),
            writer_ref=writer_ref,
            reader_ref=reader_ref,
        )

    def _get_worker_metadata(self) -> dict[str, Any]:
        try:
            sdk_version = version("iii-sdk")
        except Exception:
            sdk_version = "unknown"

        # III_WORKER_NAME carries the config.yaml entry name for managed
        # workers (set by iii-worker at spawn). Engine truth (`iii worker
        # status`/`list`) matches connections by name, so the managed identity
        # must win over the hostname:pid fallback.
        worker_name = (
            self._options.worker_name
            or os.environ.get("III_WORKER_NAME")
            or f"{platform.node()}:{os.getpid()}"
        )

        # III_NAMESPACE mirrors III_WORKER_NAME precedence: an explicit option
        # wins, then the managed env var, else None (the engine applies its
        # `default` namespace when absent).
        namespace = (
            self._options.namespace
            or os.environ.get("III_NAMESPACE")
            or None
        )

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
            "amplitude_api_key": (
                telemetry_opts.amplitude_api_key if telemetry_opts else None
            ),
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
        >>> # The queue must be declared in the iii-queue worker's queue_configs.
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


def register_worker(address: str, options: InitOptions | None = None) -> III:
    """Register the worker with a iii instance, returns a connected worker client.

    Blocks up to 30 seconds for the WebSocket connection to be established.
    If the engine is not reachable in time, a warning is logged and the
    client is returned anyway — it keeps retrying in the background and
    flushes registrations once connected. Use
    ``add_connection_state_listener`` to observe the actual transition.

    Args:
        address: WebSocket URL of the III engine (e.g. ``ws://localhost:49134``).
        options: Optional configuration for worker name, timeouts, reconnection, and OTel.

    Returns:
        An III client instance (connected, or still connecting after the
        30s wait timed out).

    Raises:
        ConnectionError: If the connection fails or exceeds max retries.

    Examples:
        >>> from iii import register_worker, InitOptions
        >>> worker = register_worker('ws://localhost:49134', InitOptions(worker_name='my-worker'))
    """
    client = III(address, options)
    client._wait_until_connected()
    return client
