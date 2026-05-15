
from __future__ import annotations

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


def _build_local_provider() -> tuple[TracerProvider, InMemorySpanExporter]:
    # `set_tracer_provider` is first-write-wins in OTel Python; tests
    # must use a local provider to avoid cross-test interference.
    local_exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(SimpleSpanProcessor(local_exporter))
    return provider, local_exporter


def test_record_span_event_writes_event_with_attributes() -> None:
    provider, local_exporter = _build_local_provider()
    tracer = provider.get_tracer("test")
    with tracer.start_as_current_span("inner") as span:
        record_span_event(
            "iii.invocation.input",
            {"iii.payload.json": '{"x":1}', "iii.payload.truncated": False},
        )
        assert span.is_recording()

    spans = local_exporter.get_finished_spans()
    assert len(spans) == 1
    events = [e for e in spans[0].events if e.name == "iii.invocation.input"]
    assert len(events) == 1
    assert events[0].attributes["iii.payload.json"] == '{"x":1}'
    assert events[0].attributes["iii.payload.truncated"] is False


def test_record_span_event_is_noop_without_active_span() -> None:
    _, local_exporter = _build_local_provider()
    record_span_event("orphan", {"k": "v"})
    assert local_exporter.get_finished_spans() == ()


def test_current_span_is_recording_inside_active_span() -> None:
    provider, _ = _build_local_provider()
    tracer = provider.get_tracer("test")
    with tracer.start_as_current_span("inner"):
        assert current_span_is_recording() is True


def test_current_span_is_recording_outside_active_span() -> None:
    assert current_span_is_recording() is False


def test_set_current_span_attribute_writes_to_active_span() -> None:
    provider, local_exporter = _build_local_provider()
    tracer = provider.get_tracer("test")

    with tracer.start_as_current_span("inner"):
        set_current_span_attribute("iii.session.id", "S-1")

    spans = local_exporter.get_finished_spans()
    assert len(spans) == 1
    assert spans[0].attributes["iii.session.id"] == "S-1"


def test_set_current_span_attribute_is_noop_outside_span() -> None:
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
    set_current_span_error("boom")
