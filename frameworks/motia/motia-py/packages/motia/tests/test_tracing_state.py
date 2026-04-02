"""Tests for OpenTelemetry instrumentation of state operations."""

import sys
from unittest.mock import MagicMock, patch

import pytest
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry.trace import StatusCode

from motia.state import StateManager

_state_mod = sys.modules["motia.state"]


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


def test_state_get_creates_span(otel_exporter, mock_iii):
    """state.get() should create a span named 'state.get' with correct attributes."""
    mock_iii.trigger.return_value = {"key": "value"}

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        sm = StateManager()
        result = sm.get("scope1", "key1")

    assert result == {"key": "value"}

    spans = otel_exporter.get_finished_spans()
    state_spans = [s for s in spans if s.name == "state::get"]
    assert len(state_spans) == 1

    span = state_spans[0]
    assert span.attributes["motia.state.scope"] == "scope1"
    assert span.attributes["motia.state.key"] == "key1"
    assert span.status.status_code == StatusCode.OK


def test_state_set_creates_span(otel_exporter, mock_iii):
    """state.set() should create a span named 'state.set' with correct attributes."""
    mock_iii.trigger.return_value = {"key": "value"}

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        sm = StateManager()
        result = sm.set("scope1", "key1", {"key": "value"})

    assert result == {"key": "value"}

    spans = otel_exporter.get_finished_spans()
    state_spans = [s for s in spans if s.name == "state::set"]
    assert len(state_spans) == 1

    span = state_spans[0]
    assert span.attributes["motia.state.scope"] == "scope1"
    assert span.attributes["motia.state.key"] == "key1"
    assert span.status.status_code == StatusCode.OK


def test_state_delete_creates_span(otel_exporter, mock_iii):
    """state.delete() should create a span named 'state.delete' with correct attributes."""
    mock_iii.trigger.return_value = None

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        sm = StateManager()
        sm.delete("scope1", "key1")

    spans = otel_exporter.get_finished_spans()
    state_spans = [s for s in spans if s.name == "state::delete"]
    assert len(state_spans) == 1

    span = state_spans[0]
    assert span.attributes["motia.state.scope"] == "scope1"
    assert span.attributes["motia.state.key"] == "key1"
    assert span.status.status_code == StatusCode.OK


def test_state_list_creates_span(otel_exporter, mock_iii):
    """state.list() should create a span named 'state.list' with correct attributes."""
    mock_iii.trigger.return_value = [{"id": "a"}, {"id": "b"}]

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        sm = StateManager()
        result = sm.list("scope1")

    assert result == [{"id": "a"}, {"id": "b"}]

    spans = otel_exporter.get_finished_spans()
    state_spans = [s for s in spans if s.name == "state::list"]
    assert len(state_spans) == 1

    span = state_spans[0]
    assert span.attributes["motia.state.scope"] == "scope1"
    assert span.status.status_code == StatusCode.OK


def test_state_list_groups_creates_span(otel_exporter, mock_iii):
    """state.list_groups() should create a span named 'state.list_groups'."""
    mock_iii.trigger.return_value = ["group1", "group2"]

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        sm = StateManager()
        result = sm.list_groups()

    assert result == ["group1", "group2"]

    spans = otel_exporter.get_finished_spans()
    state_spans = [s for s in spans if s.name == "state::list_groups"]
    assert len(state_spans) == 1

    span = state_spans[0]
    assert span.status.status_code == StatusCode.OK


def test_state_clear_creates_span(otel_exporter, mock_iii):
    """state.clear() should create a span named 'state.clear' with correct attributes."""
    mock_iii.trigger.return_value = [{"id": "item1"}, {"id": "item2"}]

    with patch.object(_state_mod, "get_instance", return_value=mock_iii):
        sm = StateManager()
        sm.clear("scope1")

    spans = otel_exporter.get_finished_spans()
    clear_spans = [s for s in spans if s.name == "state::clear"]
    assert len(clear_spans) == 1

    span = clear_spans[0]
    assert span.attributes["motia.state.scope"] == "scope1"
    assert span.status.status_code == StatusCode.OK
