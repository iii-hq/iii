// Baggage → span attribute processor.
//
// Copies an allowlisted set of OTel baggage entries from the parent context
// onto every new span as attributes at `on_start`. Installed by default in
// `initOtel` (see `./index.ts`), so every Node worker built on iii-sdk
// automatically materializes `iii.session.id`, `iii.message.id`, and
// `iii.function_id` as queryable span attributes when those entries are
// present in the propagated context.
//
// Producers (e.g. the harness wrapper in the Rust workers tree) write the
// IDs into baggage. iii-sdk's wire layer propagates baggage on every
// `iii.trigger(...)` so every downstream worker's context inherits them.
// But baggage is NOT a span attribute by default — without this processor,
// downstream worker spans hold the IDs in context but never tag their own
// spans, so `engine::traces::list` cannot query for them. This processor
// closes that gap.
//
// Mirrors the Rust implementation at
// `motia/sdk/packages/rust/iii/src/telemetry/baggage_span_processor.rs`.

import type { Context } from '@opentelemetry/api'
import { propagation } from '@opentelemetry/api'
import type { ReadableSpan, Span, SpanProcessor } from '@opentelemetry/sdk-trace-base'

/**
 * Default keys copied from baggage to span attributes.
 *
 * Aligned with the harness wrapper's baggage write set (Rust-side
 * `harness::otel::HARNESS_KEYS`). The harness-side tests pin the Rust
 * source-of-truth; the test in `tests/baggage-span-processor.test.ts`
 * pins this constant against the same literal so cross-language drift
 * is caught at CI time.
 */
export const DEFAULT_ALLOWLIST: readonly string[] = [
  'iii.session.id',
  'iii.message.id',
  'iii.function_id',
] as const

/**
 * Copies allowlisted baggage entries from the parent context onto each new
 * span as attributes at `onStart`. Composable with `BatchSpanProcessor` —
 * chain this one first so attributes are present before the batch processor
 * reads the span.
 */
export class BaggageSpanProcessor implements SpanProcessor {
  private readonly allowlist: readonly string[]

  constructor(allowlist: readonly string[] = DEFAULT_ALLOWLIST) {
    this.allowlist = allowlist
  }

  onStart(span: Span, parentContext: Context): void {
    // NoOp guard: skip baggage lookup + attribute allocation entirely
    // when the new span isn't recording (sampler dropped it, or no
    // tracer provider). `setAttribute` on a non-recording span is
    // itself a no-op, but constructing the attribute call still costs;
    // matters under high QPS across every span in every worker.
    if (!span.isRecording()) {
      return
    }

    const baggage = propagation.getBaggage(parentContext)
    if (!baggage) {
      return
    }

    for (const key of this.allowlist) {
      const entry = baggage.getEntry(key)
      if (entry) {
        span.setAttribute(key, entry.value)
      }
    }
  }

  // No-op: this processor only enriches at start, doesn't export.
  onEnd(_span: ReadableSpan): void {
    /* no-op */
  }

  async shutdown(): Promise<void> {
    /* no-op */
  }

  async forceFlush(): Promise<void> {
    /* no-op */
  }
}
