// Baggage -> span attribute processor.

import type { Context } from '@opentelemetry/api'
import { propagation } from '@opentelemetry/api'
import type { ReadableSpan, Span, SpanProcessor } from '@opentelemetry/sdk-trace-base'

/** DEFAULT_ALLOWLIST drift across languages would break worker chains;
 * lockstep tests in each SDK pin this constant at CI time. */
export const DEFAULT_ALLOWLIST: readonly string[] = [
  'iii.session.id',
  'iii.message.id',
  'iii.function_id',
] as const

export class BaggageSpanProcessor implements SpanProcessor {
  private readonly allowlist: readonly string[]

  constructor(allowlist: readonly string[] = DEFAULT_ALLOWLIST) {
    this.allowlist = allowlist
  }

  onStart(span: Span, parentContext: Context): void {
    // NoOp guard: skip allocation when sampler drops the span.
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
