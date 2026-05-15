"""Unit tests for BaggageSpanProcessor."""

from __future__ import annotations

from opentelemetry import baggage, context
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import (
    InMemorySpanExporter,
)
from opentelemetry.sdk.trace.sampling import ALWAYS_OFF

from iii.baggage_span_processor import (
    DEFAULT_ALLOWLIST,
    BaggageSpanProcessor,
)


def _build_test_provider(
    processor: BaggageSpanProcessor,
) -> tuple[TracerProvider, InMemorySpanExporter]:
    exporter = InMemorySpanExporter()
    provider = TracerProvider()
    provider.add_span_processor(processor)
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    return provider, exporter


def _attach_baggage(entries: dict[str, str]):
    ctx = context.get_current()
    for key, value in entries.items():
        ctx = baggage.set_baggage(key, value, ctx)
    return context.attach(ctx)


def _first_span_attr(exporter: InMemorySpanExporter, key: str) -> object | None:
    spans = exporter.get_finished_spans()
    if not spans:
        return None
    return spans[0].attributes.get(key) if spans[0].attributes else None


def test_copies_default_allowlist_from_baggage_to_attributes() -> None:
    provider, exporter = _build_test_provider(BaggageSpanProcessor())
    tracer = provider.get_tracer("test")

    token = _attach_baggage(
        {
            "iii.session.id": "S-1",
            "iii.message.id": "M-1",
            "iii.function.id": "auth::set_token",
        }
    )
    try:
        with tracer.start_as_current_span("inner"):
            pass
    finally:
        context.detach(token)

    assert _first_span_attr(exporter, "iii.session.id") == "S-1"
    assert _first_span_attr(exporter, "iii.message.id") == "M-1"
    assert _first_span_attr(exporter, "iii.function.id") == "auth::set_token"


def test_missing_baggage_entry_means_attribute_not_set() -> None:
    provider, exporter = _build_test_provider(BaggageSpanProcessor())
    tracer = provider.get_tracer("test")

    token = _attach_baggage({"iii.message.id": "M-only"})
    try:
        with tracer.start_as_current_span("inner"):
            pass
    finally:
        context.detach(token)

    assert _first_span_attr(exporter, "iii.message.id") == "M-only"
    assert _first_span_attr(exporter, "iii.session.id") is None
    assert _first_span_attr(exporter, "iii.function.id") is None


def test_baggage_entries_not_in_allowlist_are_dropped() -> None:
    provider, exporter = _build_test_provider(BaggageSpanProcessor())
    tracer = provider.get_tracer("test")

    token = _attach_baggage(
        {
            "iii.message.id": "M",
            "tenant.id": "t-42",
            "debug.feature_flag": "on",
        }
    )
    try:
        with tracer.start_as_current_span("inner"):
            pass
    finally:
        context.detach(token)

    assert _first_span_attr(exporter, "iii.message.id") == "M"
    assert _first_span_attr(exporter, "tenant.id") is None
    assert _first_span_attr(exporter, "debug.feature_flag") is None


def test_custom_allowlist_is_honored() -> None:
    processor = BaggageSpanProcessor(allowlist=["tenant.id", "iii.message.id"])
    provider, exporter = _build_test_provider(processor)
    tracer = provider.get_tracer("test")

    token = _attach_baggage(
        {
            "tenant.id": "t-1",
            "iii.message.id": "M",
            "iii.session.id": "S-not-copied",
        }
    )
    try:
        with tracer.start_as_current_span("inner"):
            pass
    finally:
        context.detach(token)

    assert _first_span_attr(exporter, "tenant.id") == "t-1"
    assert _first_span_attr(exporter, "iii.message.id") == "M"
    assert _first_span_attr(exporter, "iii.session.id") is None


def test_empty_parent_context_produces_no_attributes() -> None:
    provider, exporter = _build_test_provider(BaggageSpanProcessor())
    tracer = provider.get_tracer("test")

    with tracer.start_as_current_span("inner"):
        pass

    assert _first_span_attr(exporter, "iii.session.id") is None
    assert _first_span_attr(exporter, "iii.message.id") is None


def test_noop_guard_skips_processing_when_sampled_out() -> None:
    exporter = InMemorySpanExporter()
    provider = TracerProvider(sampler=ALWAYS_OFF)
    provider.add_span_processor(BaggageSpanProcessor())
    provider.add_span_processor(SimpleSpanProcessor(exporter))
    tracer = provider.get_tracer("test")

    token = _attach_baggage(
        {"iii.session.id": "S-1", "iii.message.id": "M-1"}
    )
    try:
        with tracer.start_as_current_span("inner"):
            pass
    finally:
        context.detach(token)

    assert exporter.get_finished_spans() == ()


def test_default_allowlist_matches_other_sdks() -> None:
    """DEFAULT_ALLOWLIST drift across languages would break worker chains."""
    assert tuple(DEFAULT_ALLOWLIST) == (
        "iii.session.id",
        "iii.message.id",
        "iii.function.id",
    )
