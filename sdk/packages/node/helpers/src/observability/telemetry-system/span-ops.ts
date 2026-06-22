/** High-level span operations so consumers don't need `@opentelemetry/api`. */

import { SpanStatusCode, trace, type AttributeValue, type Attributes } from '@opentelemetry/api'

/** Returns `false` when there is no active span or the sampler dropped it. */
export function currentSpanIsRecording(): boolean {
  const span = trace.getActiveSpan()
  return span ? span.isRecording() : false
}

/** No-op when the current span is not recording. */
export function setCurrentSpanAttribute(key: string, value: AttributeValue): void {
  const span = trace.getActiveSpan()
  if (!span || !span.isRecording()) return
  span.setAttribute(key, value)
}

/** No-op when there is no active span. */
export function setCurrentSpanError(message: string): void {
  const span = trace.getActiveSpan()
  if (!span) return
  span.setStatus({ code: SpanStatusCode.ERROR, message })
}

/** No-op when the current span is not recording. */
export function recordSpanEvent(name: string, attrs?: Attributes): void {
  const span = trace.getActiveSpan()
  if (!span || !span.isRecording()) return
  span.addEvent(name, attrs)
}
