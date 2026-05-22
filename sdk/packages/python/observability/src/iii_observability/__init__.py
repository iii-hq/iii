"""iii-observability: shared OTel + Logger primitives."""

__version__ = "0.13.0.dev1"

from .baggage_span_processor import DEFAULT_ALLOWLIST, BaggageSpanProcessor
from .http_instrumentation import execute_traced_request
from .logger import Logger
from .payload import (
    REDACTED_PLACEHOLDER,
    redact,
    redact_and_truncate,
    resolve_max_bytes_from_env,
)
from .reconnection import ReconnectionConfig
from .span_ops import (
    current_span_is_recording,
    record_span_event,
    set_current_span_attribute,
    set_current_span_error,
)
from .telemetry import (
    current_span_id,
    current_trace_id,
    extract_baggage,
    extract_traceparent,
    flush_otel,
    init_otel,
    inject_baggage,
    inject_traceparent,
    shutdown_otel,
    with_span,
)
from .telemetry_types import OtelConfig

__all__ = [
    "BaggageSpanProcessor",
    "DEFAULT_ALLOWLIST",
    "Logger",
    "OtelConfig",
    "REDACTED_PLACEHOLDER",
    "ReconnectionConfig",
    "current_span_id",
    "current_span_is_recording",
    "current_trace_id",
    "execute_traced_request",
    "extract_baggage",
    "extract_traceparent",
    "flush_otel",
    "init_otel",
    "inject_baggage",
    "inject_traceparent",
    "record_span_event",
    "redact",
    "redact_and_truncate",
    "resolve_max_bytes_from_env",
    "set_current_span_attribute",
    "set_current_span_error",
    "shutdown_otel",
    "with_span",
]
