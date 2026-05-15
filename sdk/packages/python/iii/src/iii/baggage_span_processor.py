"""Baggage -> span attribute processor."""

from __future__ import annotations

from typing import Sequence

from opentelemetry import baggage
from opentelemetry.context import Context
from opentelemetry.sdk.trace import ReadableSpan, Span, SpanProcessor

#: DEFAULT_ALLOWLIST drift across languages would break worker chains;
#: lockstep tests in each SDK pin this constant at CI time.
DEFAULT_ALLOWLIST: tuple[str, ...] = (
    "iii.session.id",
    "iii.message.id",
    "iii.function.id",
)


class BaggageSpanProcessor(SpanProcessor):

    def __init__(self, allowlist: Sequence[str] = DEFAULT_ALLOWLIST) -> None:
        self._allowlist: tuple[str, ...] = tuple(allowlist)

    def on_start(self, span: Span, parent_context: Context | None = None) -> None:
        # NoOp guard: skip allocation when sampler drops the span.
        if not span.is_recording():
            return

        for key in self._allowlist:
            value = baggage.get_baggage(key, parent_context)
            if value is not None:
                span.set_attribute(key, str(value))

    def on_end(self, span: ReadableSpan) -> None:  # noqa: ARG002
        pass

    def shutdown(self) -> None:
        pass

    def force_flush(self, timeout_millis: int = 30000) -> bool:  # noqa: ARG002
        return True
