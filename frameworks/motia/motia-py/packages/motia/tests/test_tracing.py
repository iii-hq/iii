"""Tests for motia.tracing – core utilities and bridge instrumentation."""

import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry import trace

from motia.tracing import (
    HAS_OTEL,
    _incoming_baggage,
    _incoming_traceparent,
    _instrumented_bridges,
    extract_parent_context,
    get_trace_id_from_span,
    get_tracer,
    inject_context,
    instrument_bridge,
    operation_span,
    step_span,
)


@pytest.fixture(autouse=True)
def otel_exporter():
    """Set up an in-memory span exporter with a fresh TracerProvider.

    Resets the global TracerProvider between tests so each test gets
    a clean provider with its own InMemorySpanExporter.
    """
    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))

    # Reset the global tracer provider guard so we can set it again
    trace._TRACER_PROVIDER_SET_ONCE = trace.Once()  # type: ignore[attr-defined]
    trace._TRACER_PROVIDER = None  # type: ignore[attr-defined]
    trace.set_tracer_provider(provider)

    yield exporter
    provider.shutdown()


@pytest.fixture(autouse=True)
def _reset_bridge_set():
    """Clear instrumented bridges set between tests."""
    _instrumented_bridges.clear()


def _make_mock_bridge():
    """Create a mock bridge with _to_dict and _handle_message."""
    bridge = SimpleNamespace()
    bridge._to_dict = lambda msg: dict(msg) if isinstance(msg, dict) else {"data": msg}
    bridge._handle_message = AsyncMock()
    return bridge


# -- Core utility tests --


def test_has_otel_is_true():
    """HAS_OTEL should be True when opentelemetry is installed."""
    assert HAS_OTEL is True


def test_get_tracer_returns_tracer():
    """get_tracer() should return a Tracer instance."""
    tracer = get_tracer()
    assert tracer is not None
    assert hasattr(tracer, "start_as_current_span")


def test_inject_and_extract_roundtrip(otel_exporter):
    """inject_context inside a span should produce a traceparent that
    extract_parent_context can consume."""
    tracer = get_tracer()
    with tracer.start_as_current_span("test-roundtrip"):
        headers = inject_context()

    assert "traceparent" in headers
    ctx = extract_parent_context(headers["traceparent"])
    assert ctx is not None


def test_get_trace_id_from_span(otel_exporter):
    """get_trace_id_from_span() inside a span should return a 32-char hex string."""
    tracer = get_tracer()
    with tracer.start_as_current_span("test-trace-id"):
        trace_id = get_trace_id_from_span()
        assert trace_id is not None
        assert len(trace_id) == 32
        # Should be valid hex
        int(trace_id, 16)


def test_get_trace_id_returns_none_outside_span():
    """get_trace_id_from_span() outside any span should return None."""
    trace_id = get_trace_id_from_span()
    assert trace_id is None


def test_extract_parent_context_returns_none_for_none():
    """extract_parent_context(None) should return None."""
    ctx = extract_parent_context(None)
    assert ctx is None


def test_step_span_creates_span(otel_exporter):
    """step_span should create a SERVER span with correct attributes."""
    with step_span("my-step", "event", custom="val") as span:
        assert span is not None

    spans = otel_exporter.get_finished_spans()
    assert len(spans) == 1
    s = spans[0]
    assert s.name == "step:my-step"
    assert s.attributes["motia.step.name"] == "my-step"
    assert s.attributes["motia.trigger.type"] == "event"
    assert s.attributes["custom"] == "val"


def test_operation_span_creates_child_span(otel_exporter):
    """operation_span inside a step_span should create a CLIENT child span."""
    with step_span("parent-step", "event") as parent:
        with operation_span("stream.emit", topic="orders") as child:
            assert child is not None

    spans = otel_exporter.get_finished_spans()
    assert len(spans) == 2
    child_span = spans[0]  # child finishes first
    parent_span = spans[1]
    assert child_span.name == "stream.emit"
    assert child_span.attributes["topic"] == "orders"
    # child should be a child of parent
    assert child_span.context.trace_id == parent_span.context.trace_id
    assert child_span.parent.span_id == parent_span.context.span_id


# -- Bridge instrumentation tests --


def test_instrument_bridge_patches_to_dict(otel_exporter):
    """instrument_bridge should inject traceparent into invokefunction messages."""
    bridge = _make_mock_bridge()
    instrument_bridge(bridge)

    tracer = get_tracer()
    with tracer.start_as_current_span("test-inject"):
        result = bridge._to_dict({"type": "invokefunction", "data": "hello"})

    assert "traceparent" in result
    assert result["traceparent"].startswith("00-")


def test_instrument_bridge_does_not_inject_for_other_messages(otel_exporter):
    """instrument_bridge should NOT inject traceparent for non-invokefunction messages."""
    bridge = _make_mock_bridge()
    instrument_bridge(bridge)

    tracer = get_tracer()
    with tracer.start_as_current_span("test-no-inject"):
        result = bridge._to_dict({"type": "registerfunction", "data": "hello"})

    assert "traceparent" not in result


@pytest.mark.asyncio
async def test_instrument_bridge_extracts_traceparent(otel_exporter):
    """instrument_bridge should extract traceparent from incoming invokefunction messages."""
    bridge = _make_mock_bridge()
    instrument_bridge(bridge)

    tp = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
    msg = json.dumps({"type": "invokefunction", "traceparent": tp, "function_id": "f1"})

    await bridge._handle_message(msg)

    assert _incoming_traceparent.get() == tp


def test_instrument_bridge_idempotent():
    """Calling instrument_bridge twice on the same instance should only patch once."""
    bridge = _make_mock_bridge()
    instrument_bridge(bridge)
    first_to_dict = bridge._to_dict

    instrument_bridge(bridge)
    assert bridge._to_dict is first_to_dict
