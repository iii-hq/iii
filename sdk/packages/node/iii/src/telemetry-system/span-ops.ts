/**
 * High-level span operations for consumers who don't want to depend on
 * `@opentelemetry/api` types directly. Mirrors the Rust SDK's
 * `telemetry::span_ops` module: same gating, same intent.
 *
 * The dispatcher uses `recordSpanEvent` to emit `iii.invocation.input` /
 * `iii.invocation.output` when `III_TRACE_PAYLOADS=1`. Worker code can
 * call it for domain events (`llm.request`, `tool.invoked`) without
 * pulling `@opentelemetry/api` as a direct dep.
 */

import { SpanStatusCode, trace, type AttributeValue, type Attributes } from '@opentelemetry/api'

/**
 * True when the current span is being recorded. Returns false when
 * there is no active span or when the tracer is a NoOp / not initialized.
 */
export function currentSpanIsRecording(): boolean {
  const span = trace.getActiveSpan()
  return span ? span.isRecording() : false
}

/**
 * Set an attribute on the current span. No-op when the current span is
 * not recording. Mirrors `iii_sdk::set_current_span_attribute` in the
 * Rust SDK so worker code can tag spans without importing OTel directly.
 */
export function setCurrentSpanAttribute(key: string, value: AttributeValue): void {
  const span = trace.getActiveSpan()
  if (!span || !span.isRecording()) return
  span.setAttribute(key, value)
}

/**
 * Mark the current span's status as Error with the given message.
 * No-op when there is no active span. Mirrors
 * `iii_sdk::set_current_span_error` in the Rust SDK.
 */
export function setCurrentSpanError(message: string): void {
  const span = trace.getActiveSpan()
  if (!span) return
  span.setStatus({ code: SpanStatusCode.ERROR, message })
}

/**
 * Record an event on the current span. No-op when the current span is
 * not recording. Attributes are a plain object; values may be strings,
 * numbers, or booleans (OTel `Attributes` type).
 *
 * Mirrors `iii_sdk::record_span_event` in the Rust SDK.
 */
export function recordSpanEvent(name: string, attrs?: Attributes): void {
  const span = trace.getActiveSpan()
  if (!span || !span.isRecording()) return
  span.addEvent(name, attrs)
}
