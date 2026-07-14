"""Baggage -> span attribute processor.

Copies EVERY baggage entry onto every span started in its scope,
unconditionally; the same contract as the upstream OTel contrib
``BaggageSpanProcessor``. There is deliberately no key filtering here: a
filtering policy baked into worker binaries has to be kept in lockstep
across every SDK language and every deployed worker, and a stale binary
silently drops newer keys. Which attributes *mean* something (e.g. the
``iii.tag.*`` trace-tag namespace, ``iii.session.*``) is a query-side
convention owned by the engine's traces API, where it can evolve without
touching workers.
"""

from __future__ import annotations

from opentelemetry import baggage
from opentelemetry.context import Context
from opentelemetry.sdk.trace import ReadableSpan, Span, SpanProcessor


class BaggageSpanProcessor(SpanProcessor):

    def on_start(self, span: Span, parent_context: Context | None = None) -> None:
        # NoOp guard: skip allocation when sampler drops the span.
        if not span.is_recording():
            return

        for key, value in baggage.get_all(parent_context).items():
            if value is None:
                continue
            span.set_attribute(key, str(value))

    def on_end(self, span: ReadableSpan) -> None:  # noqa: ARG002
        pass

    def shutdown(self) -> None:
        pass

    def force_flush(self, timeout_millis: int = 30000) -> bool:  # noqa: ARG002
        return True
