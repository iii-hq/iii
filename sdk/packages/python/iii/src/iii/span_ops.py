"""High-level span operations for consumers who don't want to depend on
``opentelemetry`` directly. Mirrors the Rust SDK's
``telemetry::span_ops`` module and the Node SDK's ``telemetry-system/span-ops``:
same gating, same intent.

The dispatcher uses :func:`record_span_event` to emit
``iii.invocation.input`` / ``iii.invocation.output`` when
``III_TRACE_PAYLOADS=1``. Worker code can call it for domain events
(``llm.request``, ``tool.invoked``) without pulling ``opentelemetry``
as a direct dependency.
"""

from __future__ import annotations

from typing import Any

from opentelemetry import trace
from opentelemetry.trace import Status, StatusCode


def current_span_is_recording() -> bool:
    """True when the current span is being recorded.

    Returns False when there is no active span or when the tracer is a
    NoOp / not initialized.
    """
    span = trace.get_current_span()
    return bool(span and span.is_recording())


def set_current_span_attribute(key: str, value: Any) -> None:
    """Set an attribute on the current span. No-op when not recording.

    Mirrors ``iii_sdk::set_current_span_attribute`` in the Rust SDK and
    ``setCurrentSpanAttribute`` in the Node SDK so worker code can tag
    spans without importing ``opentelemetry`` directly.
    """
    span = trace.get_current_span()
    if not span or not span.is_recording():
        return
    span.set_attribute(key, value)


def set_current_span_error(message: str) -> None:
    """Mark the current span's status as Error with the given message.

    No-op when there is no active span. Mirrors
    ``iii_sdk::set_current_span_error`` in the Rust SDK and
    ``setCurrentSpanError`` in the Node SDK.
    """
    span = trace.get_current_span()
    if not span:
        return
    span.set_status(Status(StatusCode.ERROR, message))


def record_span_event(name: str, attrs: dict[str, Any] | None = None) -> None:
    """Record an event on the current span. No-op when not recording.

    ``attrs`` is a plain dict of string keys to attribute values
    (strings, numbers, or booleans). Mirrors
    ``iii_sdk::record_span_event`` in the Rust SDK and
    ``recordSpanEvent`` in the Node SDK.
    """
    span = trace.get_current_span()
    if not span or not span.is_recording():
        return
    span.add_event(name, attributes=attrs or {})
