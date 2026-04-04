"""Tests for SharedEngineConnection, EngineSpanExporter, EngineLogExporter."""

import asyncio
import json
from unittest.mock import MagicMock

import pytest

from iii.telemetry_exporters import EngineLogExporter, EngineSpanExporter, SharedEngineConnection


def _make_mock_connection():
    conn = MagicMock(spec=SharedEngineConnection)
    sent = []

    def capture(prefix, payload):
        sent.append((prefix, payload))

    conn.send_threadsafe.side_effect = capture
    conn._sent = sent
    return conn


def _make_readable_span():
    """Create a minimal ReadableSpan for testing."""
    from opentelemetry.sdk.trace import TracerProvider
    from opentelemetry.sdk.trace.export import SimpleSpanProcessor
    from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter

    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    tracer = provider.get_tracer("test")
    with tracer.start_as_current_span("test-span"):
        pass
    return exporter.get_finished_spans()[0]


def test_engine_span_exporter_sends_otlp_prefix():
    conn = _make_mock_connection()
    exporter = EngineSpanExporter(conn)
    span = _make_readable_span()

    from opentelemetry.sdk.trace.export import SpanExportResult

    result = exporter.export([span])

    assert result == SpanExportResult.SUCCESS
    assert len(conn._sent) == 1
    prefix, payload = conn._sent[0]
    assert prefix == b"OTLP"
    # payload must be valid JSON with camelCase keys
    parsed = json.loads(payload.decode())
    assert "resourceSpans" in parsed


def test_engine_span_exporter_sends_hex_trace_id():
    """Exported span JSON must use lowercase hex trace_id/span_id, not base64."""
    conn = _make_mock_connection()
    exporter = EngineSpanExporter(conn)
    span = _make_readable_span()

    exporter.export([span])

    _, payload = conn._sent[0]
    parsed = json.loads(payload.decode())
    exported_span = parsed["resourceSpans"][0]["scopeSpans"][0]["spans"][0]

    expected_trace_id = format(span.context.trace_id, "032x")
    expected_span_id = format(span.context.span_id, "016x")

    assert exported_span["traceId"] == expected_trace_id, (
        f"Expected hex trace_id {expected_trace_id!r}, got {exported_span['traceId']!r}"
    )
    assert exported_span["spanId"] == expected_span_id, (
        f"Expected hex span_id {expected_span_id!r}, got {exported_span['spanId']!r}"
    )


def test_engine_span_exporter_int_attributes_are_json_numbers():
    """intValue must be a JSON number (not string) for engine serde compatibility."""
    conn = _make_mock_connection()
    exporter = EngineSpanExporter(conn)

    from opentelemetry.sdk.trace import TracerProvider
    from opentelemetry.sdk.trace.export import SimpleSpanProcessor
    from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter

    mem = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(mem))
    tracer = provider.get_tracer("test")
    with tracer.start_as_current_span("test-span", attributes={"http.status_code": 200}):
        pass
    span = mem.get_finished_spans()[0]

    exporter.export([span])
    _, payload = conn._sent[0]
    parsed = json.loads(payload.decode())
    attrs = parsed["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
    status_attr = next(a for a in attrs if a["key"] == "http.status_code")
    # Must be a JSON number, not a string — engine's OtlpAnyValue uses i64
    assert status_attr["value"]["intValue"] == 200
    assert isinstance(status_attr["value"]["intValue"], int)


def test_engine_span_exporter_returns_failure_on_error():
    conn = _make_mock_connection()
    conn.send_threadsafe.side_effect = RuntimeError("boom")
    exporter = EngineSpanExporter(conn)
    span = _make_readable_span()

    from opentelemetry.sdk.trace.export import SpanExportResult

    result = exporter.export([span])
    assert result == SpanExportResult.FAILURE


def test_engine_log_exporter_sends_logs_prefix():
    conn = _make_mock_connection()
    exporter = EngineLogExporter(conn)

    import time

    from opentelemetry._logs import LogRecord, SeverityNumber
    from opentelemetry.sdk._logs import ReadableLogRecord
    from opentelemetry.sdk.resources import Resource

    log_record = LogRecord(
        timestamp=time.time_ns(),
        observed_timestamp=time.time_ns(),
        trace_id=0,
        span_id=0,
        trace_flags=None,
        severity_text="INFO",
        severity_number=SeverityNumber.INFO,
        body="test log",
        attributes={},
    )
    record = ReadableLogRecord(log_record=log_record, resource=Resource.create({}))

    from opentelemetry.sdk._logs.export import LogExportResult

    result = exporter.export([record])

    assert result == LogExportResult.SUCCESS
    assert len(conn._sent) == 1
    prefix, payload = conn._sent[0]
    assert prefix == b"LOGS"
    parsed = json.loads(payload.decode())
    assert "resourceLogs" in parsed or "resource_logs" in parsed


@pytest.fixture
def event_loop():
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


def test_send_threadsafe_before_start_buffers_frame():
    """Frames sent before start() are buffered in pre-start deque."""
    conn = SharedEngineConnection("ws://localhost:99999")  # unreachable
    conn.send_threadsafe(b"OTLP", b"hello")
    assert len(conn._pre_start_buffer) == 1
    prefix, payload = conn._pre_start_buffer[0]
    assert prefix == b"OTLP"
    assert payload == b"hello"


def test_pre_start_buffer_drops_oldest_when_full():
    """Pre-start buffer drops oldest frame when MAX_QUEUE exceeded."""
    conn = SharedEngineConnection("ws://localhost:99999")
    for i in range(SharedEngineConnection.MAX_QUEUE + 1):
        conn.send_threadsafe(b"OTLP", str(i).encode())
    assert len(conn._pre_start_buffer) == SharedEngineConnection.MAX_QUEUE
    # Oldest (0) was dropped; newest is still present
    _, last = conn._pre_start_buffer[-1]
    assert last == str(SharedEngineConnection.MAX_QUEUE).encode()


@pytest.mark.asyncio
async def test_start_drains_pre_start_buffer_into_queue():
    """start() moves buffered frames into the asyncio queue."""
    conn = SharedEngineConnection("ws://localhost:99999")
    conn.send_threadsafe(b"OTLP", b"span1")
    conn.send_threadsafe(b"LOGS", b"log1")

    loop = asyncio.get_event_loop()

    # Patch _run to be a no-op so we don't attempt real connection
    async def _noop():
        await asyncio.sleep(9999)

    conn._run = _noop

    conn.start(loop)
    assert conn._queue is not None
    assert conn._queue.qsize() == 2
    assert len(conn._pre_start_buffer) == 0
    await conn.shutdown()
