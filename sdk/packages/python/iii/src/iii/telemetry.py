"""OpenTelemetry initialization for the III Python SDK.

Provides init_otel() / shutdown_otel() which set up distributed tracing
(via EngineSpanExporter), log export (via EngineLogExporter), and
auto-instrument urllib with rich HTTP attributes matching the Node.js SDK.
"""

from __future__ import annotations

import asyncio
import logging
import os
import uuid
from typing import Any, cast

from .telemetry_types import OtelConfig

_tracer: Any = None
_meter: Any = None
_meter_provider: Any = None
_log_provider: Any = None
_connection: Any = None  # SharedEngineConnection | None
_initialized: bool = False
_fetch_patched: bool = False

_DEFAULT_SERVICE_NAME = "iii-python-sdk"


def init_otel(
    config: OtelConfig | None = None,
    loop: asyncio.AbstractEventLoop | None = None,
) -> None:
    """Initialize OpenTelemetry. Subsequent calls are no-ops.

    Args:
        config: OTel configuration.
        loop: Running asyncio event loop. When provided, SharedEngineConnection
              starts immediately. When None, the connection is started lazily
              on first use (pre-start buffer absorbs early frames).
    """
    global _tracer, _log_provider, _connection, _initialized, _fetch_patched

    if _initialized:
        return

    cfg = config or OtelConfig()

    if cfg.enabled is not None:
        enabled = cfg.enabled
    else:
        # Enabled by default; set OTEL_ENABLED=false/0/no/off to disable
        env = os.environ.get("OTEL_ENABLED", "").lower()
        enabled = env not in ("false", "0", "no", "off")

    if not enabled:
        return

    try:
        from opentelemetry import trace
        from opentelemetry.sdk.resources import SERVICE_NAME, SERVICE_VERSION, Resource
        from opentelemetry.sdk.trace import TracerProvider
        from opentelemetry.sdk.trace.export import BatchSpanProcessor
    except ImportError as exc:
        raise ImportError(
            "opentelemetry-api and opentelemetry-sdk are required. Install with: pip install 'iii-sdk[otel]'"
        ) from exc

    service_name = cfg.service_name or os.environ.get("OTEL_SERVICE_NAME") or _DEFAULT_SERVICE_NAME
    service_version = cfg.service_version or os.environ.get("SERVICE_VERSION") or "unknown"
    service_instance_id = cfg.service_instance_id or os.environ.get("SERVICE_INSTANCE_ID") or str(uuid.uuid4())

    resource_attrs: dict[str, Any] = {
        SERVICE_NAME: service_name,
        SERVICE_VERSION: service_version,
        "service.instance.id": service_instance_id,
        "telemetry.sdk.name": "iii-python-sdk",
        "telemetry.sdk.language": "python",
    }
    service_namespace = cfg.service_namespace or os.environ.get("SERVICE_NAMESPACE")
    if service_namespace:
        resource_attrs["service.namespace"] = service_namespace

    resource = Resource.create(resource_attrs)

    # --- Span exporter ---
    from .telemetry_exporters import EngineSpanExporter, SharedEngineConnection

    ws_url = cfg.engine_ws_url or os.environ.get("III_URL") or "ws://localhost:49134"
    _connection = SharedEngineConnection(ws_url)
    if loop is not None:
        _connection.start(loop)

    span_exporter = EngineSpanExporter(_connection)
    provider = TracerProvider(resource=resource)
    provider.add_span_processor(BatchSpanProcessor(span_exporter))  # type: ignore[arg-type]
    trace.set_tracer_provider(provider)
    _tracer = trace.get_tracer("iii-python-sdk")

    # --- Metrics exporter ---
    if cfg.metrics_enabled:
        _configure_meter_provider(resource, _connection, cfg, service_name)

    # --- Log exporter ---
    logs_enabled = cfg.logs_enabled if cfg.logs_enabled is not None else True
    if logs_enabled:
        _configure_log_provider(resource, _connection, cfg)

    _initialized = True

    if cfg.fetch_instrumentation_enabled:
        _enable_fetch_instrumentation()


def _configure_meter_provider(
    resource: Any,
    connection: Any,
    cfg: OtelConfig,
    service_name: str,
) -> None:
    """Set up a global MeterProvider with EngineMetricsExporter."""
    global _meter, _meter_provider
    try:
        from opentelemetry import metrics
        from opentelemetry.sdk.metrics import MeterProvider
        from opentelemetry.sdk.metrics.export import PeriodicExportingMetricReader

        from .telemetry_exporters import EngineMetricsExporter
    except ImportError:
        logging.getLogger("iii.telemetry").warning("opentelemetry-sdk metrics not available; metrics export skipped.")
        return

    metrics_exporter = EngineMetricsExporter(connection)
    metric_reader = PeriodicExportingMetricReader(
        metrics_exporter,  # type: ignore[arg-type]
        export_interval_millis=cfg.metrics_export_interval_ms,
    )
    meter_provider = MeterProvider(resource=resource, metric_readers=[metric_reader])
    metrics.set_meter_provider(meter_provider)
    _meter_provider = meter_provider
    _meter = meter_provider.get_meter(service_name)


def _resolve_int(
    config_value: int | None,
    env_var: str,
    default: int,
    minimum: int = 0,
) -> int:
    """Resolve an integer setting: explicit config > env var > default.

    Matches the Node SDK's resolution order for cross-SDK consistency.
    """
    if config_value is not None:
        return config_value

    raw = os.environ.get(env_var)
    if raw is not None:
        try:
            val = int(raw)
            if val >= minimum:
                return val
        except (ValueError, TypeError):
            pass

    return default


def _configure_log_provider(resource: Any, connection: Any, cfg: OtelConfig) -> None:
    """Set up a global SdkLoggerProvider with EngineLogExporter."""
    global _log_provider
    try:
        from opentelemetry import _logs
        from opentelemetry.sdk._logs import LoggerProvider as SdkLoggerProvider
        from opentelemetry.sdk._logs.export import BatchLogRecordProcessor

        from .telemetry_exporters import EngineLogExporter
    except ImportError:
        logging.getLogger("iii.telemetry").warning("opentelemetry-sdk logs not available; log export skipped.")
        return

    log_exporter = EngineLogExporter(connection)

    logs_flush_interval_ms = _resolve_int(
        cfg.logs_flush_interval_ms,
        "OTEL_LOGS_FLUSH_INTERVAL_MS",
        default=100,
    )
    logs_batch_size = _resolve_int(
        cfg.logs_batch_size,
        "OTEL_LOGS_BATCH_SIZE",
        default=1,
        minimum=1,
    )

    log_provider = SdkLoggerProvider(resource=resource)
    log_provider.add_log_record_processor(
        BatchLogRecordProcessor(
            cast(Any, log_exporter),
            schedule_delay_millis=logs_flush_interval_ms,
            max_export_batch_size=logs_batch_size,
        )
    )
    _logs.set_logger_provider(log_provider)
    _log_provider = log_provider

    logging.getLogger("iii.telemetry").debug(
        "Log provider configured: flush_interval=%dms, batch_size=%d",
        logs_flush_interval_ms,
        logs_batch_size,
    )


_original_opener_open: Any = None


def _enable_fetch_instrumentation() -> None:
    """Patch urllib.request.OpenerDirector.open to create OTel CLIENT spans.

    Custom instrumentation matching the Node.js SDK's patchGlobalFetch —
    uses new OTel semantic conventions and adds rich attributes (server.address,
    url.scheme, url.path, http.response.status_code, etc.).
    """
    global _fetch_patched, _original_opener_open

    try:
        from opentelemetry import context as otel_ctx
        from opentelemetry.propagate import inject as otel_inject
        from opentelemetry.trace import SpanKind, StatusCode
    except ImportError:
        logging.getLogger("iii.telemetry").warning(
            "opentelemetry-api not installed; urllib auto-instrumentation skipped."
        )
        return

    import socket
    import urllib.request
    from urllib.parse import urlparse

    _original_opener_open = urllib.request.OpenerDirector.open
    original = _original_opener_open

    def _patched_open(self: Any, fullurl: Any, data: Any = None, timeout: Any = socket.getdefaulttimeout()) -> Any:
        tracer = get_tracer()
        if tracer is None:
            return original(self, fullurl, data, timeout)

        # Parse URL and method
        if isinstance(fullurl, str):
            url = fullurl
            method = "POST" if data is not None else "GET"
        else:
            url = fullurl.full_url
            method = fullurl.get_method()

        attrs: dict[str, Any] = {"http.request.method": method, "url.full": url}

        try:
            parsed = urlparse(url)
            if parsed.hostname:
                attrs["server.address"] = parsed.hostname
            if parsed.scheme:
                attrs["url.scheme"] = parsed.scheme
                attrs["network.protocol.name"] = "http"
            if parsed.path:
                attrs["url.path"] = parsed.path
            if parsed.port:
                attrs["server.port"] = parsed.port
            if parsed.query:
                attrs["url.query"] = parsed.query
        except Exception:
            pass

        if data is not None and isinstance(data, (bytes, bytearray)):
            attrs["http.request.body.size"] = len(data)
        if not isinstance(fullurl, str) and fullurl.has_header("Content-type"):
            attrs["http.request.header.content-type"] = fullurl.get_header("Content-type")

        span_name = f"{method} {attrs.get('url.path', '')}" if "url.path" in attrs else method

        with tracer.start_as_current_span(span_name, kind=SpanKind.CLIENT, attributes=attrs) as span:
            # Convert string URL to Request so we can inject trace context headers
            if isinstance(fullurl, str):
                fullurl = urllib.request.Request(fullurl, data)
                data = None

            carrier: dict[str, str] = {}
            otel_inject(carrier, context=otel_ctx.get_current())
            for key, value in carrier.items():
                fullurl.add_unredirected_header(key, value)

            try:
                response = original(self, fullurl, data, timeout)

                span.set_attribute("http.response.status_code", response.status)

                cl = response.headers.get("content-length")
                if cl:
                    try:
                        span.set_attribute("http.response.body.size", int(cl))
                    except ValueError:
                        pass
                ct = response.headers.get("content-type")
                if ct:
                    span.set_attribute("http.response.header.content-type", ct)

                if response.status >= 400:
                    span.set_attribute("error.type", str(response.status))
                    span.set_status(StatusCode.ERROR)
                else:
                    span.set_status(StatusCode.OK)

                return response
            except Exception as exc:
                span.set_attribute("error.type", type(exc).__name__)
                span.set_status(StatusCode.ERROR, str(exc))
                span.record_exception(exc)
                raise

    urllib.request.OpenerDirector.open = _patched_open  # type: ignore[method-assign]
    _fetch_patched = True


def shutdown_otel() -> None:
    """Shut down OTel synchronously (best-effort; does not await WS flush)."""
    _reset_state()


async def shutdown_otel_async() -> None:
    """Shut down OTel and await WebSocket connection close."""
    global _connection
    if _connection is not None:
        await _connection.shutdown()
    _reset_state()


def _shutdown_provider(provider: Any) -> None:
    """Call shutdown() on a provider, silently ignoring errors."""
    try:
        if provider is not None and hasattr(provider, "shutdown"):
            provider.shutdown()
    except Exception:
        pass


def _reset_state() -> None:
    global _tracer, _meter, _meter_provider, _log_provider, _connection, _initialized, _fetch_patched

    if _fetch_patched:
        try:
            import urllib.request

            if _original_opener_open is not None:
                urllib.request.OpenerDirector.open = _original_opener_open  # type: ignore[method-assign]
        except Exception:
            pass
        _fetch_patched = False

    if _initialized:
        try:
            from opentelemetry import trace

            _shutdown_provider(trace.get_tracer_provider())
        except Exception:
            pass
        _shutdown_provider(_meter_provider)
        _shutdown_provider(_log_provider)

    _tracer = None
    _meter = None
    _meter_provider = None
    _log_provider = None
    _connection = None
    _initialized = False


def attach_event_loop(loop: asyncio.AbstractEventLoop) -> None:
    """Wire the running asyncio event loop into the OTel connection.

    Call this from within an async context (e.g. III.connect_async()) after
    init_otel() has been called without a loop so that SharedEngineConnection
    starts sending buffered frames immediately.
    """
    if _initialized and _connection is not None and not _connection._started:
        _connection.start(loop)


def get_tracer() -> Any:
    """Return the active tracer, or None if OTel has not been initialized."""
    return _tracer


def get_meter() -> Any:
    """Return the active meter, or None if OTel metrics have not been initialized."""
    return _meter


def current_trace_id() -> str | None:
    """Return current active trace_id as 32-char hex, or None when unavailable."""
    try:
        from opentelemetry import trace

        span_ctx = trace.get_current_span().get_span_context()
        if span_ctx.is_valid:
            return format(span_ctx.trace_id, "032x")
    except ImportError:
        return None
    return None


def current_span_id() -> str | None:
    """Return current active span_id as 16-char hex, or None when unavailable."""
    try:
        from opentelemetry import trace

        span_ctx = trace.get_current_span().get_span_context()
        if span_ctx.is_valid:
            return format(span_ctx.span_id, "016x")
    except ImportError:
        return None
    return None


def is_initialized() -> bool:
    """Return True if OTel has been successfully initialized."""
    return _initialized


def get_logger() -> Any:
    """Return the active OTel logger, or None if OTel has not been initialized."""
    if not _initialized:
        return None
    try:
        from opentelemetry import _logs

        return _logs.get_logger("iii-python-sdk")
    except ImportError:
        return None


async def with_span(
    name: str,
    fn: Any,
    *,
    kind: Any = None,
    traceparent: str | None = None,
) -> Any:
    """Start a new span and run *fn(span)* within it.

    If the tracer is not initialized, *fn* is called with a no-op span
    that silently ignores attribute/event calls.

    Args:
        name: Span name.
        fn: Async callable ``(span) -> T``.
        kind: Optional ``SpanKind``. Defaults to ``INTERNAL``.
        traceparent: Optional W3C traceparent to use as parent context.

    Returns:
        The value returned by *fn*.
    """
    tracer = get_tracer()
    if tracer is None:

        class _NoopSpan:
            def set_attribute(self, *a: Any, **kw: Any) -> None: ...
            def set_attributes(self, *a: Any, **kw: Any) -> None: ...
            def add_event(self, *a: Any, **kw: Any) -> None: ...
            def set_status(self, *a: Any, **kw: Any) -> None: ...
            def record_exception(self, *a: Any, **kw: Any) -> None: ...
            def end(self) -> None: ...
            def is_recording(self) -> bool:
                return False

            def get_span_context(self) -> Any:
                return None

        return await fn(_NoopSpan())

    try:
        from opentelemetry import context as otel_ctx
        from opentelemetry.propagate import extract as otel_extract
        from opentelemetry.trace import SpanKind as _SpanKind
        from opentelemetry.trace import StatusCode

    except ImportError:
        return await fn(None)

    span_kind = kind if kind is not None else _SpanKind.INTERNAL
    parent_context = otel_ctx.get_current()
    if traceparent:
        parent_context = otel_extract({"traceparent": traceparent})

    with tracer.start_as_current_span(name, kind=span_kind, context=parent_context) as span:
        try:
            result = await fn(span)
            span.set_status(StatusCode.OK)
            return result
        except Exception as exc:
            span.set_status(StatusCode.ERROR, str(exc))
            span.record_exception(exc)
            raise


def inject_traceparent() -> str | None:
    """Inject the current trace context into a W3C traceparent header string."""
    try:
        from opentelemetry import context as otel_ctx
        from opentelemetry.propagate import inject as otel_inject

        carrier: dict[str, str] = {}
        otel_inject(carrier, context=otel_ctx.get_current())
        return carrier.get("traceparent")
    except ImportError:
        return None


def extract_traceparent(traceparent: str) -> Any:
    """Extract a trace context from a W3C traceparent header string."""
    try:
        from opentelemetry.propagate import extract as otel_extract

        return otel_extract({"traceparent": traceparent})
    except ImportError:
        return None


def inject_baggage() -> str | None:
    """Inject the current baggage into a W3C baggage header string."""
    try:
        from opentelemetry import context as otel_ctx
        from opentelemetry.propagate import inject as otel_inject

        carrier: dict[str, str] = {}
        otel_inject(carrier, context=otel_ctx.get_current())
        return carrier.get("baggage")
    except ImportError:
        return None


def extract_baggage(baggage: str) -> Any:
    """Extract baggage from a W3C baggage header string."""
    try:
        from opentelemetry.propagate import extract as otel_extract

        return otel_extract({"baggage": baggage})
    except ImportError:
        return None


def extract_context(traceparent: str | None = None, baggage: str | None = None) -> Any:
    """Extract both trace context and baggage from their respective headers."""
    try:
        from opentelemetry.propagate import extract as otel_extract

        carrier: dict[str, str] = {}
        if traceparent:
            carrier["traceparent"] = traceparent
        if baggage:
            carrier["baggage"] = baggage
        return otel_extract(carrier)
    except ImportError:
        return None


def get_baggage_entry(key: str) -> str | None:
    """Get a baggage entry value from the current context."""
    try:
        from opentelemetry import baggage as otel_baggage

        val = otel_baggage.get_baggage(key)
        return str(val) if val is not None else None
    except ImportError:
        return None


def set_baggage_entry(key: str, value: str) -> Any:
    """Set a baggage entry in the current context. Returns the new context."""
    try:
        from opentelemetry import baggage as otel_baggage

        ctx = otel_baggage.set_baggage(key, value)
        return ctx
    except ImportError:
        return None


def remove_baggage_entry(key: str) -> Any:
    """Remove a baggage entry from the current context. Returns the new context."""
    try:
        from opentelemetry import baggage as otel_baggage

        return otel_baggage.remove_baggage(key)
    except ImportError:
        return None


def get_all_baggage() -> dict[str, str]:
    """Get all baggage entries from the current context."""
    try:
        from opentelemetry import baggage as otel_baggage

        entries = otel_baggage.get_all()
        return {k: str(v) for k, v in entries.items()} if entries else {}
    except ImportError:
        return {}
