"""Tests for OpenTelemetry instrumentation of stream operations."""

from unittest.mock import MagicMock, patch

import pytest
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry.trace import StatusCode

from motia.streams import Stream


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
def mock_iii():
    """Create a mock III SDK instance."""
    iii = MagicMock()
    iii.trigger = MagicMock()
    return iii


def test_stream_get_creates_span(otel_exporter, mock_iii):
    """stream.get() should create a span named 'stream.get' with correct attributes."""
    mock_iii.trigger.return_value = {"id": "item1", "value": "data"}

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("my-stream")
        result = stream.get("group1", "item1")

    assert result == {"id": "item1", "value": "data"}

    spans = otel_exporter.get_finished_spans()
    stream_spans = [s for s in spans if s.name == "stream::get"]
    assert len(stream_spans) == 1

    span = stream_spans[0]
    assert span.attributes["motia.stream.name"] == "my-stream"
    assert span.attributes["motia.stream.group_id"] == "group1"
    assert span.attributes["motia.stream.item_id"] == "item1"
    assert span.status.status_code == StatusCode.OK


def test_stream_set_creates_span(otel_exporter, mock_iii):
    """stream.set() should create a span named 'stream.set' with correct attributes."""
    mock_iii.trigger.return_value = {"id": "item1", "value": "hello"}

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("my-stream")
        result = stream.set("group1", "item1", {"value": "hello"})

    assert result == {"id": "item1", "value": "hello"}

    spans = otel_exporter.get_finished_spans()
    stream_spans = [s for s in spans if s.name == "stream::set"]
    assert len(stream_spans) == 1

    span = stream_spans[0]
    assert span.attributes["motia.stream.name"] == "my-stream"
    assert span.attributes["motia.stream.group_id"] == "group1"
    assert span.attributes["motia.stream.item_id"] == "item1"
    assert span.status.status_code == StatusCode.OK


def test_stream_delete_creates_span(otel_exporter, mock_iii):
    """stream.delete() should create a span named 'stream.delete' with correct attributes."""
    mock_iii.trigger.return_value = None

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("my-stream")
        stream.delete("group1", "item1")

    spans = otel_exporter.get_finished_spans()
    stream_spans = [s for s in spans if s.name == "stream::delete"]
    assert len(stream_spans) == 1

    span = stream_spans[0]
    assert span.attributes["motia.stream.name"] == "my-stream"
    assert span.attributes["motia.stream.group_id"] == "group1"
    assert span.attributes["motia.stream.item_id"] == "item1"
    assert span.status.status_code == StatusCode.OK


def test_stream_get_group_creates_span(otel_exporter, mock_iii):
    """stream.get_group() should create a span named 'stream.list' with correct attributes."""
    mock_iii.trigger.return_value = [{"id": "a"}, {"id": "b"}]

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("my-stream")
        result = stream.get_group("group1")

    assert result == [{"id": "a"}, {"id": "b"}]

    spans = otel_exporter.get_finished_spans()
    stream_spans = [s for s in spans if s.name == "stream::list"]
    assert len(stream_spans) == 1

    span = stream_spans[0]
    assert span.attributes["motia.stream.name"] == "my-stream"
    assert span.attributes["motia.stream.group_id"] == "group1"
    assert span.status.status_code == StatusCode.OK


def test_stream_list_groups_creates_span(otel_exporter, mock_iii):
    """stream.list_groups() should create a span named 'stream.list_groups' with correct attributes."""
    mock_iii.trigger.return_value = ["group1", "group2"]

    with patch("motia.streams.get_instance", return_value=mock_iii):
        stream = Stream("my-stream")
        result = stream.list_groups()

    assert result == ["group1", "group2"]

    spans = otel_exporter.get_finished_spans()
    stream_spans = [s for s in spans if s.name == "stream::list_groups"]
    assert len(stream_spans) == 1

    span = stream_spans[0]
    assert span.attributes["motia.stream.name"] == "my-stream"
    assert span.status.status_code == StatusCode.OK
