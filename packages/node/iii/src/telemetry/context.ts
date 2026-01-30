/**
 * Trace context and baggage propagation utilities.
 */

import { trace, context, propagation, type Context } from '@opentelemetry/api'

/**
 * Extract the current trace ID from the active span context.
 */
export function currentTraceId(): string | undefined {
  const span = trace.getActiveSpan()
  if (span) {
    const spanContext = span.spanContext()
    if (spanContext.traceId && spanContext.traceId !== '00000000000000000000000000000000') {
      return spanContext.traceId
    }
  }
  return undefined
}

/**
 * Extract the current span ID from the active span context.
 */
export function currentSpanId(): string | undefined {
  const span = trace.getActiveSpan()
  if (span) {
    const spanContext = span.spanContext()
    if (spanContext.spanId && spanContext.spanId !== '0000000000000000') {
      return spanContext.spanId
    }
  }
  return undefined
}

/**
 * Inject the current trace context into a W3C traceparent header string.
 */
export function injectTraceparent(): string | undefined {
  const carrier: Record<string, string> = {}
  propagation.inject(context.active(), carrier)
  return carrier['traceparent']
}

/**
 * Extract a trace context from a W3C traceparent header string.
 */
export function extractTraceparent(traceparent: string): Context {
  const carrier: Record<string, string> = { traceparent }
  return propagation.extract(context.active(), carrier)
}

/**
 * Inject the current baggage into a W3C baggage header string.
 */
export function injectBaggage(): string | undefined {
  const carrier: Record<string, string> = {}
  propagation.inject(context.active(), carrier)
  return carrier['baggage']
}

/**
 * Extract baggage from a W3C baggage header string.
 */
export function extractBaggage(baggage: string): Context {
  const carrier: Record<string, string> = { baggage }
  return propagation.extract(context.active(), carrier)
}

/**
 * Extract both trace context and baggage from their respective headers.
 */
export function extractContext(traceparent?: string, baggage?: string): Context {
  const carrier: Record<string, string> = {}
  if (traceparent) {
    carrier['traceparent'] = traceparent
  }
  if (baggage) {
    carrier['baggage'] = baggage
  }
  return propagation.extract(context.active(), carrier)
}

/**
 * Get a baggage entry from the current context.
 */
export function getBaggageEntry(key: string): string | undefined {
  const bag = propagation.getBaggage(context.active())
  return bag?.getEntry(key)?.value
}

/**
 * Set a baggage entry in the current context.
 */
export function setBaggageEntry(key: string, value: string): Context {
  let bag = propagation.getBaggage(context.active()) ?? propagation.createBaggage()
  bag = bag.setEntry(key, { value })
  return propagation.setBaggage(context.active(), bag)
}

/**
 * Remove a baggage entry from the current context.
 */
export function removeBaggageEntry(key: string): Context {
  const bag = propagation.getBaggage(context.active())
  if (!bag) {
    return context.active()
  }
  const newBag = bag.removeEntry(key)
  return propagation.setBaggage(context.active(), newBag)
}

/**
 * Get all baggage entries from the current context.
 */
export function getAllBaggage(): Record<string, string> {
  const bag = propagation.getBaggage(context.active())
  if (!bag) {
    return {}
  }
  const entries: Record<string, string> = {}
  for (const [key, entry] of bag.getAllEntries()) {
    entries[key] = entry.value
  }
  return entries
}
