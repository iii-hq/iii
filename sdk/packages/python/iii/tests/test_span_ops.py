"""Tests for `record_span_event` / `current_span_is_recording` helpers."""

from __future__ import annotations

import pytest
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter

from opentelemetry.trace import StatusCode

from iii.span_ops import (
    current_span_is_recording,
    record_span_event,
    set_current_span_attribute,
    set_current_span_error,
)


@pytest.fixture()
def exporter():
    """Install a fresh in-memory span pipeline for each test."""
    span_exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(span_exporter))
    trace.set_tracer_provider(provider)
    yield span_exporter
    provider.shutdown()


def test_record_span_event_writes_event_with_attributes(exporter) -> None:
    tracer = trace.get_tracer("test")
    with tracer.start_as_current_span("inner") as span:
        record_span_event(
            "iii.invocation.input",
            {"iii.payload.json": '{"x":1}', "iii.payload.truncated": False},
        )
        assert span.is_recording()

    spans = exporter.get_finished_spans()
    assert len(spans) == 1
    events = [e for e in spans[0].events if e.name == "iii.invocation.input"]
    assert len(events) == 1
    assert events[0].attributes["iii.payload.json"] == '{"x":1}'
    assert events[0].attributes["iii.payload.truncated"] is False


def test_record_span_event_is_noop_without_active_span(exporter) -> None:
    # No span on the active context → no-op.
    record_span_event("orphan", {"k": "v"})
    assert exporter.get_finished_spans() == ()


def test_current_span_is_recording_inside_active_span(exporter) -> None:
    tracer = trace.get_tracer("test")
    with tracer.start_as_current_span("inner"):
        assert current_span_is_recording() is True


def test_current_span_is_recording_outside_active_span(exporter) -> None:
    # No span on the active context → the default no-op span returns False.
    # Pins the negative path so a regression that flips the default return
    # (e.g. dropping the `bool(span and ...)` short-circuit) is caught.
    assert current_span_is_recording() is False


def _build_local_provider() -> tuple[TracerProvider, InMemorySpanExporter]:
    # Local provider — bypasses the global `trace.set_tracer_provider`
    # which only succeeds on its first call per process (OTel Python
    # caches the first-installed provider). Tests that assert exporter
    # contents must read from the local provider's exporter, not the
    # global one. Matches the pattern used in
    # `test_baggage_span_processor.py::_build_test_provider`.
    local_exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(local_exporter))
    return provider, local_exporter


def test_set_current_span_attribute_writes_to_active_span() -> None:
    provider, local_exporter = _build_local_provider()
    tracer = provider.get_tracer("test")

    # `start_as_current_span` attaches the new span onto the active
    # context — `set_current_span_attribute` reads through
    # `trace.get_current_span()` and sees it regardless of which
    # provider built it.
    with tracer.start_as_current_span("inner"):
        set_current_span_attribute("iii.session.id", "S-1")

    spans = local_exporter.get_finished_spans()
    assert len(spans) == 1
    assert spans[0].attributes["iii.session.id"] == "S-1"


def test_set_current_span_attribute_is_noop_outside_span() -> None:
    # No span on the active context. Helper must short-circuit and emit
    # nothing — also must not raise.
    _, local_exporter = _build_local_provider()
    set_current_span_attribute("orphan.key", "value")
    assert local_exporter.get_finished_spans() == ()


def test_set_current_span_error_marks_status_error() -> None:
    provider, local_exporter = _build_local_provider()
    tracer = provider.get_tracer("test")

    with tracer.start_as_current_span("inner"):
        set_current_span_error("boom")

    spans = local_exporter.get_finished_spans()
    assert len(spans) == 1
    assert spans[0].status.status_code == StatusCode.ERROR
    assert spans[0].status.description == "boom"


def test_set_current_span_error_is_safe_outside_span() -> None:
    # No span on the active context. Helper must not raise.
    set_current_span_error("boom")
