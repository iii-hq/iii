from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider

from iii.telemetry import current_span_id, current_trace_id


def test_trace_helpers_follow_the_active_span() -> None:
    provider = TracerProvider()
    trace.set_tracer_provider(provider)
    tracer = provider.get_tracer("test")

    with tracer.start_as_current_span("helper-span") as span:
        span_ctx = span.get_span_context()
        assert current_trace_id() == format(span_ctx.trace_id, "032x")
        assert current_span_id() == format(span_ctx.span_id, "016x")
