"""Logger implementation for the III SDK."""

from __future__ import annotations

import logging
import time
from typing import Any

log = logging.getLogger("iii.logger")

_SEVERITY_MAP = {
    "info": ("INFO", 9),  # SeverityNumber.INFO
    "warn": ("WARN", 13),  # SeverityNumber.WARN
    "error": ("ERROR", 17),  # SeverityNumber.ERROR
    "debug": ("DEBUG", 5),  # SeverityNumber.DEBUG
}


def is_initialized() -> bool:
    """Return True if OTel has been initialized (importable without circular dep)."""
    try:
        from .telemetry import is_initialized as _is_init

        return _is_init()
    except ImportError:
        return False


class Logger:
    """Structured logger that emits logs as OpenTelemetry LogRecords.

    Every log call automatically captures the active trace and span context,
    correlating your logs with distributed traces without any manual wiring.
    When OTel is not initialized, Logger gracefully falls back to Python
    ``logging``.

    Pass structured data as the second argument to any log method. Using a
    dict of key-value pairs (instead of string interpolation) lets you
    filter, aggregate, and build dashboards in your observability backend.

    Examples:
        >>> from iii import Logger
        >>> logger = Logger()
        >>>
        >>> # Basic logging — trace context is injected automatically
        >>> logger.info('Worker connected')
        >>>
        >>> # Structured context for dashboards and alerting
        >>> logger.info('Order processed', {'order_id': 'ord_123', 'amount': 49.99, 'currency': 'USD'})
        >>> logger.warn('Retry attempt', {'attempt': 3, 'max_retries': 5, 'endpoint': '/api/charge'})
        >>> logger.error('Payment failed', {'order_id': 'ord_123', 'gateway': 'stripe', 'error_code': 'card_declined'})
    """

    def __init__(
        self,
        trace_id: str | None = None,
        service_name: str | None = None,
        span_id: str | None = None,
    ) -> None:
        self._trace_id = trace_id
        self._service_name = service_name or ""
        self._span_id = span_id

    def _emit_otel(self, level: str, message: str, data: Any = None) -> bool:
        """Emit an OTel LogRecord. Returns True if emitted, False if OTel not active."""
        if not is_initialized():
            return False
        try:
            from opentelemetry import _logs, trace
            from opentelemetry._logs import LogRecord, SeverityNumber

            severity_text, severity_num = _SEVERITY_MAP[level]
            otel_logger = _logs.get_logger("iii.logger")
            attrs: dict[str, Any] = {"service.name": self._service_name}
            if data is not None:
                attrs["log.data"] = data

            span_ctx = trace.get_current_span().get_span_context()

            if self._trace_id is not None:
                trace_id = int(self._trace_id, 16)
            elif span_ctx.is_valid:
                trace_id = span_ctx.trace_id
            else:
                trace_id = 0

            if self._span_id is not None:
                span_id = int(self._span_id, 16)
            elif span_ctx.is_valid:
                span_id = span_ctx.span_id
            else:
                span_id = 0

            trace_flags = span_ctx.trace_flags if span_ctx.is_valid else trace.TraceFlags(0)

            record = LogRecord(
                timestamp=time.time_ns(),
                observed_timestamp=time.time_ns(),
                severity_text=severity_text,
                severity_number=SeverityNumber(severity_num),
                body=message,
                attributes=attrs,
                trace_id=trace_id,
                span_id=span_id,
                trace_flags=trace_flags,
            )
            otel_logger.emit(record)
            return True
        except Exception:
            return False

    def _emit(self, level: str, message: str, data: Any = None) -> None:
        """Emit a log message via OTel, or Python logging as fallback."""
        if self._emit_otel(level, message, data):
            return
        _LOG_METHODS = {
            "info": log.info,
            "warn": log.warning,
            "error": log.error,
            "debug": log.debug,
        }
        log_fn = _LOG_METHODS.get(level, log.info)
        log_fn("[%s] %s", self._service_name, message, extra={"data": data})

    def info(self, message: str, data: Any = None) -> None:
        """Log an info-level message.

        Args:
            message: Human-readable log message.
            data: Structured context attached as OTel log attributes.
                Use dicts of key-value pairs to enable filtering and
                aggregation in your observability backend (e.g. Grafana,
                Datadog, New Relic).

        Examples:
            >>> logger.info('Order processed', {'order_id': 'ord_123', 'status': 'completed'})
        """
        self._emit("info", message, data)

    def warn(self, message: str, data: Any = None) -> None:
        """Log a warning-level message.

        Args:
            message: Human-readable log message.
            data: Structured context attached as OTel log attributes.
                Use dicts of key-value pairs to enable filtering and
                aggregation in your observability backend (e.g. Grafana,
                Datadog, New Relic).

        Examples:
            >>> logger.warn('Retry attempt', {'attempt': 3, 'max_retries': 5, 'endpoint': '/api/charge'})
        """
        self._emit("warn", message, data)

    def error(self, message: str, data: Any = None) -> None:
        """Log an error-level message.

        Args:
            message: Human-readable log message.
            data: Structured context attached as OTel log attributes.
                Use dicts of key-value pairs to enable filtering and
                aggregation in your observability backend (e.g. Grafana,
                Datadog, New Relic).

        Examples:
            >>> logger.error('Payment failed', {
            ...     'order_id': 'ord_123',
            ...     'gateway': 'stripe',
            ...     'error_code': 'card_declined',
            ... })
        """
        self._emit("error", message, data)

    def debug(self, message: str, data: Any = None) -> None:
        """Log a debug-level message.

        Args:
            message: Human-readable log message.
            data: Structured context attached as OTel log attributes.
                Use dicts of key-value pairs to enable filtering and
                aggregation in your observability backend (e.g. Grafana,
                Datadog, New Relic).

        Examples:
            >>> logger.debug('Cache lookup', {'key': 'user:42', 'hit': False})
        """
        self._emit("debug", message, data)
