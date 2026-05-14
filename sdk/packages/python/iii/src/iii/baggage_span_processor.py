"""Baggage → span attribute processor for the Python iii-sdk.

Copies an allowlisted set of OTel baggage entries from the parent context
onto every new span as attributes at ``on_start``. Installed by default in
``init_otel`` (see ``telemetry.py``), so every Python worker built on
iii-sdk automatically materializes ``iii.session.id``, ``iii.message.id``,
and ``iii.function_id`` as queryable span attributes when those entries
are present in the propagated context.

Producers (e.g. the Rust-side harness wrapper) write the IDs into baggage.
iii-sdk's wire layer propagates baggage on every ``iii.trigger(...)`` so
every downstream worker's context inherits them. But baggage is NOT a
span attribute by default — without this processor, downstream worker
spans hold the IDs in context but never tag their own spans, so
``engine::traces::list`` cannot query for them. This processor closes
that gap.

Mirrors the Rust implementation at
``motia/sdk/packages/rust/iii/src/telemetry/baggage_span_processor.rs``.
"""

from __future__ import annotations

from typing import Sequence

from opentelemetry import baggage
from opentelemetry.context import Context
from opentelemetry.sdk.trace import ReadableSpan, Span, SpanProcessor


#: Default keys copied from baggage to span attributes.
#:
#: Aligned with the harness wrapper's baggage write set (Rust-side
#: ``harness::otel::HARNESS_KEYS``). The harness-side tests pin the Rust
#: source-of-truth; the test in ``tests/test_baggage_span_processor.py``
#: pins this constant against the same literal so cross-language drift
#: is caught at CI time.
DEFAULT_ALLOWLIST: tuple[str, ...] = (
    "iii.session.id",
    "iii.message.id",
    "iii.function_id",
)


class BaggageSpanProcessor(SpanProcessor):
    """Copy allowlisted baggage entries from the parent context onto each
    new span as attributes at ``on_start``.

    Composable with ``BatchSpanProcessor`` — chain this one first so
    attributes are present before the batch processor reads the span.
    """

    def __init__(self, allowlist: Sequence[str] = DEFAULT_ALLOWLIST) -> None:
        self._allowlist: tuple[str, ...] = tuple(allowlist)

    def on_start(self, span: Span, parent_context: Context | None = None) -> None:
        # NoOp guard: skip baggage lookup + attribute allocation entirely
        # when the new span isn't recording (sampler dropped it, or no
        # tracer provider). `set_attribute` on a non-recording span is
        # itself a no-op, but the per-call loop still costs CPU; matters
        # under high QPS across every span in every worker.
        if not span.is_recording():
            return

        for key in self._allowlist:
            value = baggage.get_baggage(key, parent_context)
            if value is not None:
                span.set_attribute(key, str(value))

    # ReadableSpan typing per the SpanProcessor abstract base class.
    def on_end(self, span: ReadableSpan) -> None:  # noqa: ARG002
        # No-op: this processor only enriches at start, doesn't export.
        pass

    def shutdown(self) -> None:
        # No-op: no buffered work.
        pass

    def force_flush(self, timeout_millis: int = 30000) -> bool:  # noqa: ARG002
        return True
