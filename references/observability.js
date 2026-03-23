/**
 * Pattern: Observability
 * Comparable to: Datadog, Grafana, Honeycomb, OpenTelemetry SDK
 *
 * iii has built-in OpenTelemetry support for traces, metrics, and logs.
 * This file shows how to configure the telemetry pipeline, create custom
 * spans and metrics, propagate trace context across function calls, listen
 * for log events, and cleanly shut down the exporter.
 *
 * How-to references:
 *   - Telemetry & observability: https://iii.dev/docs/advanced/telemetry
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

// ---------------------------------------------------------------------------
// 1. SDK initialization with OpenTelemetry config
// ---------------------------------------------------------------------------
const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'observability',
  otel: {
    enabled: true,
    serviceName: 'my-service',
    serviceVersion: '1.2.0',
    metricsEnabled: true,
  },
})

// ---------------------------------------------------------------------------
// 2. Custom spans — wrap an operation in a named span for tracing
// iii.withSpan(name, options, callback) creates a child span under the
// current trace context. The span is automatically closed when the
// callback completes or throws.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::process' }, async (data) => {
  const logger = new Logger()

  const result = await iii.withSpan('validate-order', { attributes: { orderId: data.order_id } }, async () => {
    logger.info('Validating order inside span', { orderId: data.order_id })

    if (!data.items?.length) {
      throw new Error('Empty cart')
    }

    return { valid: true, itemCount: data.items.length }
  })

  // Nested spans for sub-operations
  const total = await iii.withSpan('calculate-total', {}, async () => {
    return data.items.reduce((sum, item) => sum + item.price * item.qty, 0)
  })

  await iii.withSpan('persist-order', { attributes: { total } }, async () => {
    await iii.trigger({
      function_id: 'state::set',
      payload: {
        scope: 'orders',
        key: data.order_id,
        value: { _key: data.order_id, total, status: 'confirmed' },
      },
    })
  })

  return { order_id: data.order_id, total, validated: result.valid }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::process',
  config: { api_path: '/orders/process', http_method: 'POST' },
})

// ---------------------------------------------------------------------------
// 3. Custom metrics — counters and histograms via getMeter()
// ---------------------------------------------------------------------------
const meter = iii.getMeter()

const orderCounter = meter.createCounter('orders.processed', {
  description: 'Total number of orders processed',
})

const latencyHistogram = meter.createHistogram('orders.latency_ms', {
  description: 'Order processing latency in milliseconds',
  unit: 'ms',
})

iii.registerFunction({ id: 'orders::with-metrics' }, async (data) => {
  const start = Date.now()

  // ... order processing logic
  const result = { order_id: data.order_id, status: 'complete' }

  // Record metrics
  orderCounter.add(1, { status: 'success', region: data.region || 'us-east-1' })
  latencyHistogram.record(Date.now() - start, { endpoint: '/orders' })

  return result
})

// ---------------------------------------------------------------------------
// 4. Trace context propagation
// Access the current trace ID, inject traceparent headers for outbound HTTP
// calls, and attach baggage for cross-service context.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'orders::call-external' }, async (data) => {
  const logger = new Logger()

  // Read current trace ID for correlation
  const traceId = iii.currentTraceId()
  logger.info('Current trace', { traceId })

  // Build headers with W3C traceparent for downstream services
  const headers = {}
  iii.injectTraceparent(headers) // adds 'traceparent' header
  iii.injectBaggage(headers, { 'user.id': data.user_id }) // adds 'baggage' header

  // Use these headers when calling external services
  // e.g. fetch('https://api.partner.com/verify', { headers })

  return { traceId, propagated: true }
})

// ---------------------------------------------------------------------------
// 5. Log listener — subscribe to all log events for external forwarding
// ---------------------------------------------------------------------------
iii.onLog((logEntry) => {
  // logEntry shape: { level, message, attributes, timestamp, traceId, spanId }
  // Forward to external system (Datadog, Splunk, etc.)
  if (logEntry.level === 'error') {
    // e.g. externalLogger.error(logEntry.message, logEntry.attributes)
  }
})

// ---------------------------------------------------------------------------
// 6. Structured logging with trace correlation
// Logger automatically attaches trace/span IDs when otel is enabled.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'debug::log-demo' }, async (data) => {
  const logger = new Logger()

  logger.info('Processing request', { requestId: data.id })
  logger.warn('Slow query detected', { query: data.query, duration_ms: 1200 })
  logger.error('Unexpected state', { expected: 'active', actual: data.status })

  return { logged: true }
})

// ---------------------------------------------------------------------------
// 7. Disable telemetry — useful for local development or testing
// ---------------------------------------------------------------------------
// const iiiNoTelemetry = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
//   workerName: 'observability-no-otel',
//   otel: {
//     enabled: false,
//   },
// })

// ---------------------------------------------------------------------------
// 8. Clean shutdown — flush pending spans and metrics on process exit
// ---------------------------------------------------------------------------
process.on('SIGTERM', async () => {
  await iii.shutdown_otel()
  process.exit(0)
})

process.on('SIGINT', async () => {
  await iii.shutdown_otel()
  process.exit(0)
})
