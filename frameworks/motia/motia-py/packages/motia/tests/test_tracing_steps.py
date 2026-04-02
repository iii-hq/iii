"""Tests for OpenTelemetry instrumentation of step execution."""

import re
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry.trace import StatusCode

from motia.types import ApiResponse, ApiTrigger, QueueTrigger, StepConfig


@pytest.fixture
def otel_exporter():
    """Set up an in-memory span exporter with a fresh TracerProvider."""
    trace._TRACER_PROVIDER_SET_ONCE = trace.Once()  # type: ignore[attr-defined]
    trace._TRACER_PROVIDER = None  # type: ignore[attr-defined]
    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    trace.set_tracer_provider(provider)
    yield exporter
    exporter.clear()
    provider.shutdown()


@pytest.fixture
def mock_bridge():
    """Create a mock bridge for step registration."""
    bridge = MagicMock()
    bridge.call = AsyncMock()
    bridge.register_function = MagicMock()
    bridge.register_trigger = MagicMock()
    return bridge


@pytest.mark.asyncio
async def test_api_handler_creates_span(otel_exporter, mock_bridge):
    """Register API step, call handler, verify step:name span with motia.trigger.type=http."""
    from motia.runtime import Motia

    config = StepConfig(
        name="my-api-step",
        triggers=[ApiTrigger(type="http", path="/test", method="GET")],
    )

    async def handler(req, ctx):
        return ApiResponse(status=200, body={"ok": True})

    with patch("motia.runtime.get_instance", return_value=mock_bridge):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

        # Get the registered handler
        api_handler = mock_bridge.register_function.call_args_list[0][0][1]
        response_writer = MagicMock()
        response_writer.send_message_async = AsyncMock()
        response_writer.close = MagicMock()
        response_writer.stream = MagicMock()
        response_writer.stream.write = MagicMock()
        request_body_reader = MagicMock()

        # Call the handler with a mock request
        result = await api_handler(
            {
                "method": "GET",
                "path": "/test",
                "path_params": {},
                "query_params": {},
                "body": None,
                "headers": {},
                "response": response_writer,
                "request_body": request_body_reader,
            }
        )

    assert result["status_code"] == 200

    spans = otel_exporter.get_finished_spans()
    assert len(spans) >= 1

    step_spans = [s for s in spans if s.name == "step:my-api-step"]
    assert len(step_spans) == 1

    span = step_spans[0]
    assert span.attributes["motia.step.name"] == "my-api-step"
    assert span.attributes["motia.trigger.type"] == "http"
    assert span.status.status_code == StatusCode.OK


@pytest.mark.asyncio
async def test_event_handler_creates_span(otel_exporter, mock_bridge):
    """Register queue step, call handler, verify step:name span with motia.trigger.type=queue."""
    from motia.runtime import Motia

    config = StepConfig(
        name="my-queue-step",
        triggers=[QueueTrigger(type="queue", topic="test-topic")],
    )

    async def handler(input_data, ctx):
        return {"processed": True}

    with patch("motia.runtime.get_instance", return_value=mock_bridge):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

        # Get the registered handler
        event_handler = mock_bridge.register_function.call_args_list[0][0][1]

        # Call the handler with event data
        await event_handler({"topic": "test-topic", "data": "hello"})

    spans = otel_exporter.get_finished_spans()
    assert len(spans) >= 1

    step_spans = [s for s in spans if s.name == "step:my-queue-step"]
    assert len(step_spans) == 1

    span = step_spans[0]
    assert span.attributes["motia.step.name"] == "my-queue-step"
    assert span.attributes["motia.trigger.type"] == "queue"
    assert span.status.status_code == StatusCode.OK


@pytest.mark.asyncio
async def test_handler_error_records_on_span(otel_exporter, mock_bridge):
    """Register step with handler that raises, verify span has ERROR status."""
    from motia.runtime import Motia

    config = StepConfig(
        name="error-step",
        triggers=[QueueTrigger(type="queue", topic="fail-topic")],
    )

    async def handler(input_data, ctx):
        raise ValueError("something went wrong")

    with patch("motia.runtime.get_instance", return_value=mock_bridge):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

        event_handler = mock_bridge.register_function.call_args_list[0][0][1]

        with pytest.raises(ValueError, match="something went wrong"):
            await event_handler({"data": "boom"})

    spans = otel_exporter.get_finished_spans()
    step_spans = [s for s in spans if s.name == "step:error-step"]
    assert len(step_spans) == 1

    span = step_spans[0]
    assert span.status.status_code == StatusCode.ERROR
    assert "something went wrong" in span.status.description


@pytest.mark.asyncio
async def test_trace_id_uses_otel_trace_id(otel_exporter, mock_bridge):
    """Register API step, capture ctx.trace_id in handler, verify it is 32 hex chars."""
    from motia.runtime import Motia

    captured_trace_id = None

    config = StepConfig(
        name="trace-id-step",
        triggers=[ApiTrigger(type="http", path="/trace", method="GET")],
    )

    async def handler(req, ctx):
        nonlocal captured_trace_id
        captured_trace_id = ctx.trace_id
        return ApiResponse(status=200, body={"trace_id": ctx.trace_id})

    with patch("motia.runtime.get_instance", return_value=mock_bridge):
        motia = Motia()
        motia.add_step(config, "steps/test_step.py", handler)

        api_handler = mock_bridge.register_function.call_args_list[0][0][1]
        response_writer = MagicMock()
        response_writer.send_message_async = AsyncMock()
        response_writer.close = MagicMock()
        response_writer.stream = MagicMock()
        response_writer.stream.write = MagicMock()
        request_body_reader = MagicMock()
        await api_handler(
            {
                "method": "GET",
                "path": "/trace",
                "path_params": {},
                "query_params": {},
                "body": None,
                "headers": {},
                "response": response_writer,
                "request_body": request_body_reader,
            }
        )

    assert captured_trace_id is not None
    # OTel trace IDs are 32 hex characters
    assert len(captured_trace_id) == 32
    assert re.match(r"^[0-9a-f]{32}$", captured_trace_id)
