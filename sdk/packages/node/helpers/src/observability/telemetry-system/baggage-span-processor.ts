// Baggage -> span attribute processor.
//
// Copies EVERY baggage entry onto every span started in its scope,
// unconditionally — the same contract as the upstream OTel contrib
// BaggageSpanProcessor. There is deliberately no key filtering here: a
// filtering policy baked into worker binaries has to be kept in lockstep
// across every SDK language and every deployed worker, and a stale binary
// silently drops newer keys. Which attributes *mean* something (e.g. the
// `iii.tag.*` trace-tag namespace, `iii.session.*`) is a query-side
// convention owned by the engine's traces API, where it can evolve without
// touching workers.

import type { Context } from '@opentelemetry/api'
import { propagation } from '@opentelemetry/api'
import type { ReadableSpan, Span, SpanProcessor } from '@opentelemetry/sdk-trace-base'

export class BaggageSpanProcessor implements SpanProcessor {
  onStart(span: Span, parentContext: Context): void {
    // NoOp guard: skip allocation when sampler drops the span.
    if (!span.isRecording()) {
      return
    }

    const baggage = propagation.getBaggage(parentContext)
    if (!baggage) {
      return
    }

    for (const [key, entry] of baggage.getAllEntries()) {
      span.setAttribute(key, entry.value)
    }
  }

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
