"""Tests for exception recording on function invocation spans."""

import pytest
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter

from iii.iii import III
from iii.iii import _TraceContextError


@pytest.mark.asyncio
async def test_invoke_with_otel_context_records_exception_with_stacktrace():
    """When a handler raises, the invocation span should contain
    an 'exception' event with exception.stacktrace attribute."""
    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))

    # Patch get_tracer to return a tracer from our test provider
    import opentelemetry.trace as trace_api

    original_get_tracer = trace_api.get_tracer
    trace_api.get_tracer = lambda name, **kwargs: provider.get_tracer(name)

    try:
        client = III.__new__(III)
        client._functions = {}
        client._pending = {}
        client._queue = []
        client._running = False

        async def failing_handler(data):
            raise ValueError("test invocation error")

        with pytest.raises(_TraceContextError) as exc_info:
            await client._invoke_with_otel_context(failing_handler, {"key": "value"}, None, None)
        assert isinstance(exc_info.value.__cause__, ValueError)
        assert "test invocation error" in str(exc_info.value.__cause__)

        spans = exporter.get_finished_spans()
        assert len(spans) >= 1, "expected at least 1 span"

        span = spans[0]
        exc_events = [e for e in span.events if e.name == "exception"]
        assert len(exc_events) >= 1, "expected at least 1 exception event"

        exc_event = exc_events[0]
        attrs = exc_event.attributes
        assert "exception.type" in attrs
        assert "exception.message" in attrs
        assert "exception.stacktrace" in attrs

        stacktrace = attrs["exception.stacktrace"]
        assert "ValueError" in stacktrace
        assert "test invocation error" in stacktrace
    finally:
        trace_api.get_tracer = original_get_tracer
        provider.shutdown()


@pytest.mark.asyncio
async def test_invoke_with_otel_context_success_no_exception():
    """When a handler succeeds, no exception event should be recorded."""
    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(exporter))

    import opentelemetry.trace as trace_api

    original_get_tracer = trace_api.get_tracer
    trace_api.get_tracer = lambda name, **kwargs: provider.get_tracer(name)

    try:
        client = III.__new__(III)
        client._functions = {}
        client._pending = {}
        client._queue = []
        client._running = False

        async def success_handler(data):
            return {"result": "ok"}

        result, traceparent = await client._invoke_with_otel_context(success_handler, {"key": "value"}, None, None)

        assert result == {"result": "ok"}

        spans = exporter.get_finished_spans()
        assert len(spans) >= 1, "expected at least 1 span"

        span = spans[0]
        exc_events = [e for e in span.events if e.name == "exception"]
        assert len(exc_events) == 0, "successful span should not have exception events"
    finally:
        trace_api.get_tracer = original_get_tracer
        provider.shutdown()
