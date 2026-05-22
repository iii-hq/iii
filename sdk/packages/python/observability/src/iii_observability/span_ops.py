"""High-level span operations so consumers don't need ``opentelemetry``."""

from __future__ import annotations

from typing import Any

from opentelemetry import trace
from opentelemetry.trace import Status, StatusCode


def current_span_is_recording() -> bool:
    """Returns ``False`` when there is no active span or the sampler dropped it."""
    span = trace.get_current_span()
    return bool(span and span.is_recording())


def set_current_span_attribute(key: str, value: Any) -> None:
    """No-op when the current span is not recording."""
    span = trace.get_current_span()
    if not span or not span.is_recording():
        return
    span.set_attribute(key, value)


def set_current_span_error(message: str) -> None:
    """No-op when there is no active span."""
    span = trace.get_current_span()
    if not span:
        return
    span.set_status(Status(StatusCode.ERROR, message))


def record_span_event(name: str, attrs: dict[str, Any] | None = None) -> None:
    """No-op when the current span is not recording."""
    span = trace.get_current_span()
    if not span or not span.is_recording():
        return
    span.add_event(name, attributes=attrs or {})
