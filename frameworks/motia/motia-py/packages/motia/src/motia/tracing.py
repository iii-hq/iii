"""OpenTelemetry instrumentation for Motia framework.

Provides distributed tracing support using OpenTelemetry.
When opentelemetry packages are installed (via `pip install motia[otel]`),
spans are automatically created for step execution, stream/state operations,
and emit calls. When not installed, all functions are no-ops.
"""

import contextvars
import json
import logging
from contextlib import contextmanager
from typing import Any, Generator

log = logging.getLogger("motia.tracing")

try:
    from opentelemetry import trace
    from opentelemetry.trace import SpanKind, StatusCode
    from opentelemetry.trace.propagation.tracecontext import TraceContextTextMapPropagator

    HAS_OTEL = True
except ImportError:
    HAS_OTEL = False

_incoming_traceparent: contextvars.ContextVar[str | None] = contextvars.ContextVar(
    "motia_incoming_traceparent", default=None
)
_incoming_baggage: contextvars.ContextVar[str | None] = contextvars.ContextVar("motia_incoming_baggage", default=None)

_propagator = TraceContextTextMapPropagator() if HAS_OTEL else None
_instrumented_bridges: set[int] = set()


def get_tracer() -> Any:
    """Get the motia tracer. Returns None if OTEL is not available."""
    if not HAS_OTEL:
        return None
    return trace.get_tracer("motia", "0.0.6")


def extract_parent_context(traceparent: str | None, baggage: str | None = None) -> Any:
    """Extract OTEL context from W3C traceparent/baggage headers."""
    if not HAS_OTEL or not _propagator or not traceparent:
        return None
    carrier: dict[str, str] = {"traceparent": traceparent}
    if baggage:
        carrier["baggage"] = baggage
    return _propagator.extract(carrier=carrier)


def inject_context() -> dict[str, str]:
    """Inject current OTEL span context into W3C traceparent/baggage headers."""
    if not HAS_OTEL or not _propagator:
        return {}
    carrier: dict[str, str] = {}
    _propagator.inject(carrier=carrier)
    return carrier


def get_trace_id_from_span() -> str | None:
    """Get the current OTEL trace ID as a 32-char hex string, or None."""
    if not HAS_OTEL:
        return None
    span = trace.get_current_span()
    if span is None:
        return None
    ctx = span.get_span_context()
    if ctx and ctx.trace_id and ctx.trace_id != 0:
        return format(ctx.trace_id, "032x")
    return None


@contextmanager
def step_span(
    step_name: str,
    trigger_type: str,
    **attributes: Any,
) -> Generator[Any, None, None]:
    """Create a SERVER span for step execution using incoming traceparent."""
    tracer = get_tracer()
    if not tracer:
        yield None
        return

    parent_ctx = extract_parent_context(
        _incoming_traceparent.get(),
        _incoming_baggage.get(),
    )

    with tracer.start_as_current_span(
        f"step:{step_name}",
        context=parent_ctx,
        kind=SpanKind.SERVER,
        attributes={
            "motia.step.name": step_name,
            "motia.trigger.type": trigger_type,
            **{k: v for k, v in attributes.items() if v is not None},
        },
    ) as span:
        yield span


@contextmanager
def operation_span(
    operation: str,
    **attributes: Any,
) -> Generator[Any, None, None]:
    """Create a CLIENT child span for an internal operation (stream, state, emit)."""
    tracer = get_tracer()
    if not tracer:
        yield None
        return

    with tracer.start_as_current_span(
        operation,
        kind=SpanKind.CLIENT,
        attributes={k: v for k, v in attributes.items() if v is not None},
    ) as span:
        yield span


def record_exception(span: Any, exc: Exception) -> None:
    """Record an exception on a span."""
    if HAS_OTEL and span is not None:
        span.set_status(StatusCode.ERROR, str(exc))
        span.record_exception(exc)


def set_span_ok(span: Any) -> None:
    """Set span status to OK."""
    if HAS_OTEL and span is not None:
        span.set_status(StatusCode.OK)


def instrument_bridge(bridge_instance: Any) -> None:
    """Monkey-patch the III bridge to propagate W3C trace context.

    Patches:
    - _to_dict: Injects traceparent/baggage into outgoing invokefunction messages
    - _handle_message: Extracts traceparent/baggage from incoming invokefunction messages

    Safe to call multiple times - only instruments each bridge instance once.
    """
    if not HAS_OTEL:
        return

    bridge_id = id(bridge_instance)
    if bridge_id in _instrumented_bridges:
        return
    _instrumented_bridges.add(bridge_id)

    original_to_dict = bridge_instance._to_dict
    original_handle_message = bridge_instance._handle_message

    def patched_to_dict(msg: Any) -> dict[str, Any]:
        data: dict[str, Any] = original_to_dict(msg)
        msg_type = data.get("type")
        if msg_type is not None and hasattr(msg_type, "value"):
            msg_type = msg_type.value
        if msg_type == "invokefunction":
            headers = inject_context()
            if "traceparent" in headers:
                data["traceparent"] = headers["traceparent"]
            if "baggage" in headers:
                data["baggage"] = headers["baggage"]
        return data

    async def patched_handle_message(raw: str | bytes) -> None:
        data = json.loads(raw if isinstance(raw, str) else raw.decode())
        msg_type = data.get("type")
        if msg_type == "invokefunction":
            _incoming_traceparent.set(data.get("traceparent"))
            _incoming_baggage.set(data.get("baggage"))
        await original_handle_message(raw)

    bridge_instance._to_dict = patched_to_dict
    bridge_instance._handle_message = patched_handle_message
    log.info("Bridge instrumented for OpenTelemetry trace context propagation")
