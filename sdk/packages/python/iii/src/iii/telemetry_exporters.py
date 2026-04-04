"""WebSocket-based OTel exporters for the III Engine.

Spans   → binary WS frame: b"OTLP" + OTLP JSON bytes
Logs    → binary WS frame: b"LOGS" + OTLP JSON bytes
Metrics → binary WS frame: b"MTRC" + OTLP JSON bytes
"""

from __future__ import annotations

import asyncio
import json
import logging
import random
from collections import defaultdict, deque
from typing import TYPE_CHECKING, Any, Sequence

if TYPE_CHECKING:
    from opentelemetry.sdk._logs.export import LogExportResult
    from opentelemetry.sdk.metrics.export import MetricExportResult
    from opentelemetry.sdk.trace.export import SpanExportResult

log = logging.getLogger("iii.telemetry_exporters")


class SharedEngineConnection:
    """Dedicated asyncio WebSocket connection for OTel telemetry data.

    Thread-safe: send_threadsafe() may be called from any thread (e.g. the
    OTel BatchSpanProcessor background thread). Frames sent before start()
    are buffered and flushed once the connection is established.
    """

    MAX_QUEUE: int = 1000

    def __init__(self, url: str) -> None:
        self._url = url
        self._loop: asyncio.AbstractEventLoop | None = None
        self._task: asyncio.Task | None = None  # type: ignore[type-arg]
        self._queue: asyncio.Queue | None = None  # type: ignore[type-arg]
        self._pre_start_buffer: deque[tuple[bytes, bytes]] = deque(maxlen=self.MAX_QUEUE)
        self._started = False

    def start(self, loop: asyncio.AbstractEventLoop) -> None:
        """Start the connection on the given event loop. Call once after connect()."""
        self._loop = loop
        self._queue = asyncio.Queue(maxsize=self.MAX_QUEUE)
        # Drain pre-start buffer into asyncio queue
        while self._pre_start_buffer:
            try:
                self._queue.put_nowait(self._pre_start_buffer.popleft())
            except asyncio.QueueFull:
                log.warning("[OTel] Queue full during pre-start drain, dropping message")
                break
        self._task = loop.create_task(self._run())
        self._started = True

    def send_threadsafe(self, prefix: bytes, payload: bytes) -> None:
        """Enqueue a binary frame from any thread.

        If called before start(), frames are buffered in _pre_start_buffer.
        When the queue is full, the message is dropped with a warning.
        """
        if not self._started or self._loop is None or self._queue is None:
            self._pre_start_buffer.append((prefix, payload))
            return

        def _try_put() -> None:
            try:
                self._queue.put_nowait((prefix, payload))  # type: ignore[union-attr]
            except asyncio.QueueFull:
                log.warning("[OTel] Telemetry queue full, dropping message")

        self._loop.call_soon_threadsafe(_try_put)

    async def _run(self) -> None:
        """Main reconnect loop — runs as an asyncio Task."""
        import websockets  # noqa: PLC0415

        delay = 1.0
        while True:
            try:
                async with websockets.connect(self._url) as ws:
                    log.debug("OTel WS connected to %s", self._url)
                    delay = 1.0
                    while True:
                        assert self._queue is not None
                        prefix, payload = await self._queue.get()
                        await ws.send(prefix + payload)
            except asyncio.CancelledError:
                return
            except Exception as exc:
                jittered_delay = delay * (1 + random.uniform(-0.3, 0.3))
                log.warning("OTel WS disconnected (%s), retrying in %.1fs", exc, jittered_delay)
                await asyncio.sleep(jittered_delay)
                delay = min(delay * 2, 30.0)

    async def shutdown(self) -> None:
        """Cancel the connection task."""
        if self._task:
            self._task.cancel()
            try:
                if self._loop is not None and self._loop is asyncio.get_running_loop():
                    await self._task
            except asyncio.CancelledError:
                pass


def _attr_value(value: Any) -> "dict[str, Any]":
    """Convert a Python value to an OTLP JSON attribute value dict."""
    if isinstance(value, bool):
        return {"boolValue": value}
    if isinstance(value, int):
        return {"intValue": value}
    if isinstance(value, float):
        return {"doubleValue": value}
    if isinstance(value, bytes):
        return {"bytesValue": value.hex()}
    if isinstance(value, str):
        return {"stringValue": value}
    if isinstance(value, (list, tuple)):
        return {"arrayValue": {"values": [_attr_value(v) for v in value]}}
    if isinstance(value, dict):
        return {"kvlistValue": {"values": [{"key": k, "value": _attr_value(v)} for k, v in value.items()]}}
    return {"stringValue": str(value)}


def _attrs_to_otlp(attrs: Any) -> "list[dict[str, Any]]":
    """Convert an OTel attributes mapping to an OTLP JSON attribute list."""
    if not attrs:
        return []
    return [{"key": k, "value": _attr_value(v)} for k, v in attrs.items()]


def _serialize_spans(spans: Sequence[Any]) -> bytes:
    """Serialize ReadableSpans to OTLP JSON with lowercase-hex trace/span IDs.

    Matches the format produced by the Node.js JsonTraceSerializer.serializeRequest().
    The III Engine stores IDs as lowercase hex (matching Rust's Display impl), so
    we must NOT use protobuf MessageToJson which base64-encodes bytes fields.
    """
    resource_map: "dict[int, Any]" = {}
    scope_map: dict[int, dict[tuple[str, str], list[Any]]] = defaultdict(lambda: defaultdict(list))

    for span in spans:
        r_id = id(span.resource)
        if r_id not in resource_map:
            resource_map[r_id] = span.resource
        scope = span.instrumentation_scope
        scope_key = (scope.name or "", scope.version or "")
        scope_map[r_id][scope_key].append(span)

    resource_spans = []
    for r_id, resource in resource_map.items():
        scope_spans = []
        for (scope_name, scope_version), span_list in scope_map[r_id].items():
            spans_json = []
            for span in span_list:
                span_json: "dict[str, Any]" = {
                    "traceId": format(span.context.trace_id, "032x"),
                    "spanId": format(span.context.span_id, "016x"),
                    "name": span.name,
                    "kind": span.kind.value,
                    "startTimeUnixNano": str(span.start_time or 0),
                    "endTimeUnixNano": str(span.end_time or 0),
                    "attributes": _attrs_to_otlp(span.attributes),
                    "events": [
                        {
                            "name": e.name,
                            "timeUnixNano": str(e.timestamp),
                            "attributes": _attrs_to_otlp(e.attributes),
                        }
                        for e in span.events
                    ],
                    "links": [
                        {
                            "traceId": format(lnk.context.trace_id, "032x"),
                            "spanId": format(lnk.context.span_id, "016x"),
                            "attributes": _attrs_to_otlp(lnk.attributes),
                        }
                        for lnk in span.links
                    ],
                    "status": {
                        "code": span.status.status_code.value,
                        "message": span.status.description or "",
                    },
                }
                if span.parent is not None:
                    span_json["parentSpanId"] = format(span.parent.span_id, "016x")
                spans_json.append(span_json)

            scope_spans.append(
                {
                    "scope": {"name": scope_name, "version": scope_version},
                    "spans": spans_json,
                }
            )

        resource_spans.append(
            {
                "resource": {"attributes": _attrs_to_otlp(resource.attributes)},
                "scopeSpans": scope_spans,
            }
        )

    return json.dumps({"resourceSpans": resource_spans}).encode()


def _serialize_logs(batch: Sequence[Any]) -> bytes:
    """Serialize log records to OTLP JSON with lowercase-hex trace/span IDs.

    Matches the format produced by the Node.js JsonLogsSerializer.serializeRequest().
    """
    resource_map: "dict[int, Any]" = {}
    scope_map: dict[int, dict[tuple[str, str], list[Any]]] = defaultdict(lambda: defaultdict(list))

    for record in batch:
        resource = getattr(record, "resource", None)
        scope = getattr(record, "instrumentation_scope", None)
        r_id = id(resource)
        if r_id not in resource_map:
            resource_map[r_id] = resource
        scope_key = (
            (scope.name or "") if scope else "",
            (scope.version or "") if scope else "",
        )
        scope_map[r_id][scope_key].append(record)

    resource_logs = []
    for r_id, resource in resource_map.items():
        scope_logs = []
        for (scope_name, scope_version), records in scope_map[r_id].items():
            log_records_json = []
            for record in records:
                lr = getattr(record, "log_record", record)
                trace_id: int = getattr(lr, "trace_id", 0) or 0
                span_id: int = getattr(lr, "span_id", 0) or 0
                body_val = getattr(lr, "body", None)
                body = str(body_val) if body_val is not None else ""

                log_json: "dict[str, Any]" = {
                    "timeUnixNano": str(getattr(lr, "timestamp", 0) or 0),
                    "observedTimeUnixNano": str(getattr(lr, "observed_timestamp", 0) or 0),
                    "severityNumber": getattr(getattr(lr, "severity_number", None), "value", 0),
                    "severityText": getattr(lr, "severity_text", "") or "",
                    "body": {"stringValue": body},
                    "attributes": _attrs_to_otlp(getattr(lr, "attributes", None)),
                }
                if trace_id:
                    log_json["traceId"] = format(trace_id, "032x")
                if span_id:
                    log_json["spanId"] = format(span_id, "016x")
                log_records_json.append(log_json)

            scope_logs.append(
                {
                    "scope": {"name": scope_name, "version": scope_version},
                    "logRecords": log_records_json,
                }
            )

        resource_attrs = getattr(resource, "attributes", None) if resource else None
        resource_logs.append(
            {
                "resource": {"attributes": _attrs_to_otlp(resource_attrs)},
                "scopeLogs": scope_logs,
            }
        )

    return json.dumps({"resourceLogs": resource_logs}).encode()


class EngineSpanExporter:
    """SpanExporter that sends OTLP JSON over the engine WebSocket connection."""

    def __init__(self, connection: SharedEngineConnection) -> None:
        self._connection = connection

    def export(self, spans: Sequence[Any]) -> SpanExportResult:
        from opentelemetry.sdk.trace.export import SpanExportResult

        try:
            json_bytes = _serialize_spans(spans)
            self._connection.send_threadsafe(b"OTLP", json_bytes)
            return SpanExportResult.SUCCESS
        except Exception:
            log.exception("EngineSpanExporter.export failed")
            return SpanExportResult.FAILURE

    def shutdown(self) -> None:
        pass

    def force_flush(self, timeout_millis: int = 30_000) -> bool:
        return True


class EngineLogExporter:
    """LogExporter that sends OTLP JSON over the engine WebSocket connection."""

    def __init__(self, connection: SharedEngineConnection) -> None:
        self._connection = connection

    def export(self, batch: Sequence[Any]) -> LogExportResult:
        from opentelemetry.sdk._logs.export import LogExportResult

        try:
            json_bytes = _serialize_logs(batch)
            self._connection.send_threadsafe(b"LOGS", json_bytes)
            return LogExportResult.SUCCESS
        except Exception:
            log.exception("EngineLogExporter.export failed")
            return LogExportResult.FAILURE

    def shutdown(self) -> None:
        pass

    def force_flush(self, timeout_millis: int = 30_000) -> bool:
        return True


PREFIX_METRICS = b"MTRC"


def _serialize_metrics(metrics_data: Any) -> bytes:
    """Serialize SDK MetricsData to OTLP JSON (ExportMetricsServiceRequest).

    Matches the format produced by the Node.js JsonMetricsSerializer.serializeRequest().
    """
    resource_metrics = []

    for resource_metric in metrics_data.resource_metrics:
        resource = resource_metric.resource
        scope_metrics_json = []

        for scope_metric in resource_metric.scope_metrics:
            scope = scope_metric.scope
            metrics_json = []

            for metric in scope_metric.metrics:
                metric_json: dict[str, Any] = {
                    "name": metric.name,
                    "description": metric.description or "",
                    "unit": metric.unit or "",
                }

                data = metric.data
                data_points_json = []

                for pt in data.data_points:
                    dp: dict[str, Any] = {
                        "attributes": _attrs_to_otlp(pt.attributes if hasattr(pt, "attributes") else None),
                        "startTimeUnixNano": str(pt.start_time_unix_nano if hasattr(pt, "start_time_unix_nano") else 0),
                        "timeUnixNano": str(pt.time_unix_nano if hasattr(pt, "time_unix_nano") else 0),
                    }

                    # NumberDataPoint: has value as int or float
                    if hasattr(pt, "value"):
                        val = pt.value
                        if isinstance(val, int):
                            dp["asInt"] = str(val)
                        else:
                            dp["asDouble"] = val

                    # HistogramDataPoint
                    if hasattr(pt, "bucket_counts"):
                        dp["count"] = str(pt.count) if hasattr(pt, "count") else "0"
                        dp["sum"] = pt.sum if hasattr(pt, "sum") else 0
                        dp["bucketCounts"] = [str(c) for c in pt.bucket_counts]
                        dp["explicitBounds"] = list(pt.explicit_bounds) if hasattr(pt, "explicit_bounds") else []
                        dp["min"] = pt.min if hasattr(pt, "min") else 0
                        dp["max"] = pt.max if hasattr(pt, "max") else 0

                    data_points_json.append(dp)

                # Determine data type from the SDK data class name
                data_type_name = type(data).__name__
                if "Histogram" in data_type_name:
                    metric_json["histogram"] = {
                        "dataPoints": data_points_json,
                        "aggregationTemporality": _temporality_value(data),
                    }
                elif "Sum" in data_type_name:
                    metric_json["sum"] = {
                        "dataPoints": data_points_json,
                        "aggregationTemporality": _temporality_value(data),
                        "isMonotonic": getattr(data, "is_monotonic", False),
                    }
                else:
                    # Gauge
                    metric_json["gauge"] = {
                        "dataPoints": data_points_json,
                    }

                metrics_json.append(metric_json)

            scope_metrics_json.append(
                {
                    "scope": {
                        "name": scope.name if scope else "",
                        "version": scope.version if scope else "",
                    },
                    "metrics": metrics_json,
                }
            )

        resource_metrics.append(
            {
                "resource": {
                    "attributes": _attrs_to_otlp(resource.attributes if resource else None),
                },
                "scopeMetrics": scope_metrics_json,
            }
        )

    return json.dumps({"resourceMetrics": resource_metrics}).encode()


def _temporality_value(data: Any) -> int:
    """Extract the AggregationTemporality int value from SDK data."""
    temporality = getattr(data, "aggregation_temporality", None)
    if temporality is not None:
        return temporality.value if hasattr(temporality, "value") else int(temporality)
    return 1  # AGGREGATION_TEMPORALITY_DELTA default


class EngineMetricsExporter:
    """MetricExporter that sends OTLP JSON over the engine WebSocket connection.

    Implements the PushMetricExporter interface from opentelemetry.sdk.metrics.export.
    """

    def __init__(self, connection: SharedEngineConnection) -> None:
        self._connection = connection
        # Required by PeriodicExportingMetricReader
        self._preferred_temporality: dict[Any, Any] = {}
        self._preferred_aggregation: dict[Any, Any] = {}

    def export(
        self,
        metrics_data: Any,
        timeout_millis: float = 10_000,
        **kwargs: Any,
    ) -> MetricExportResult:
        from opentelemetry.sdk.metrics.export import MetricExportResult

        try:
            json_bytes = _serialize_metrics(metrics_data)
            self._connection.send_threadsafe(PREFIX_METRICS, json_bytes)
            return MetricExportResult.SUCCESS
        except Exception:
            log.exception("EngineMetricsExporter.export failed")
            return MetricExportResult.FAILURE

    def shutdown(self, timeout_millis: float = 30_000, **kwargs: Any) -> None:
        pass

    def force_flush(self, timeout_millis: float = 10_000) -> bool:
        return True
