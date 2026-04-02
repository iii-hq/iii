/**
 * OpenTelemetry initialization for the III Node SDK.
 *
 * This module provides trace, metrics, and log export to the III Engine
 * via a shared WebSocket connection using OTLP JSON format.
 */

import { Resource } from '@opentelemetry/resources'
import { ATTR_SERVICE_NAME } from '@opentelemetry/semantic-conventions'
import { randomUUID } from 'node:crypto'
import {
  trace,
  context,
  propagation,
  SpanKind,
  SpanStatusCode,
  metrics,
  type Span,
  type Context,
  type Tracer,
  type Meter,
} from '@opentelemetry/api'
import { BatchSpanProcessor } from '@opentelemetry/sdk-trace-base'
import { MeterProvider, PeriodicExportingMetricReader } from '@opentelemetry/sdk-metrics'
import { CompositePropagator, W3CBaggagePropagator, W3CTraceContextPropagator } from '@opentelemetry/core'
import { NodeTracerProvider } from '@opentelemetry/sdk-trace-node'
import { registerInstrumentations } from '@opentelemetry/instrumentation'
import { LoggerProvider, BatchLogRecordProcessor } from '@opentelemetry/sdk-logs'
import { type Logger, SeverityNumber } from '@opentelemetry/api-logs'

import {
  type OtelConfig,
  DEFAULT_OTEL_CONFIG,
  parseBoolEnv,
  ATTR_SERVICE_VERSION,
  ATTR_SERVICE_NAMESPACE,
  ATTR_SERVICE_INSTANCE_ID,
} from './types'
import { SharedEngineConnection } from './connection'
import { EngineSpanExporter, EngineMetricsExporter, EngineLogExporter } from './exporters'
import { extractTraceparent } from './context'
import { patchGlobalFetch, unpatchGlobalFetch } from './fetch-instrumentation'
import { parseIntegerEnv, parseNumberEnv } from './utils'

// Re-export everything from submodules
export * from './types'
export * from './context'

// Module-level state
let sharedConnection: SharedEngineConnection | null = null
let tracerProvider: NodeTracerProvider | null = null
let meterProvider: MeterProvider | null = null
let loggerProvider: LoggerProvider | null = null
let tracer: Tracer | null = null
let meter: Meter | null = null
let logger: Logger | null = null
let serviceName: string = 'iii-node-iii'

/**
 * Initialize OpenTelemetry with the given configuration.
 * This should be called once at application startup.
 */
export function initOtel(config: OtelConfig = {}): void {
  const enabled = config.enabled ?? parseBoolEnv(process.env.OTEL_ENABLED, DEFAULT_OTEL_CONFIG.enabled)

  if (!enabled) {
    console.debug(
      '[OTel] OpenTelemetry is disabled. To enable, remove OTEL_ENABLED=false or set enabled: true in config.',
    )
    return
  }

  // Configure service identity
  serviceName = config.serviceName ?? process.env.OTEL_SERVICE_NAME ?? DEFAULT_OTEL_CONFIG.serviceName
  const serviceVersion = config.serviceVersion ?? process.env.SERVICE_VERSION ?? DEFAULT_OTEL_CONFIG.serviceVersion
  const serviceNamespace = config.serviceNamespace ?? process.env.SERVICE_NAMESPACE
  const serviceInstanceId = config.serviceInstanceId ?? process.env.SERVICE_INSTANCE_ID ?? randomUUID()
  const engineWsUrl = config.engineWsUrl ?? process.env.III_URL ?? DEFAULT_OTEL_CONFIG.engineWsUrl

  // Build resource attributes
  const resourceAttributes: Record<string, string> = {
    [ATTR_SERVICE_NAME]: serviceName,
    [ATTR_SERVICE_VERSION]: serviceVersion,
    [ATTR_SERVICE_INSTANCE_ID]: serviceInstanceId,
  }
  if (serviceNamespace) {
    resourceAttributes[ATTR_SERVICE_NAMESPACE] = serviceNamespace
  }
  const resource = new Resource(resourceAttributes)

  // Create shared WebSocket connection
  sharedConnection = new SharedEngineConnection(engineWsUrl, config.reconnectionConfig)

  // Initialize tracer
  const spanExporter = new EngineSpanExporter(sharedConnection)
  tracerProvider = new NodeTracerProvider({
    resource,
    spanProcessors: [new BatchSpanProcessor(spanExporter)],
  })

  // Register W3C Trace Context and Baggage propagators
  propagation.setGlobalPropagator(
    new CompositePropagator({
      propagators: [new W3CTraceContextPropagator(), new W3CBaggagePropagator()],
    }),
  )

  tracerProvider.register()
  tracer = trace.getTracer(serviceName)

  console.debug(`[OTel] Traces initialized: engine=${engineWsUrl}, service=${serviceName}`)

  // Initialize metrics (enabled by default, opt-out via config or env)
  const metricsEnabled =
    config.metricsEnabled ?? parseBoolEnv(process.env.OTEL_METRICS_ENABLED, DEFAULT_OTEL_CONFIG.metricsEnabled)

  if (metricsEnabled) {
    const metricsExporter = new EngineMetricsExporter(sharedConnection)
    const exportIntervalMs = config.metricsExportIntervalMs ?? DEFAULT_OTEL_CONFIG.metricsExportIntervalMs

    const metricReader = new PeriodicExportingMetricReader({
      exporter: metricsExporter,
      exportIntervalMillis: exportIntervalMs,
    })

    meterProvider = new MeterProvider({
      resource,
      readers: [metricReader],
    })

    metrics.setGlobalMeterProvider(meterProvider)
    meter = meterProvider.getMeter(serviceName)

    console.debug(`[OTel] Metrics initialized: interval=${exportIntervalMs}ms`)
  }

  // Register user-provided instrumentations AFTER providers are set up
  const instrumentations = [...(config.instrumentations ?? [])]
  if (instrumentations.length > 0) {
    registerInstrumentations({
      instrumentations,
      tracerProvider,
      meterProvider: meterProvider ?? undefined,
    })
    console.debug(`[OTel] Instrumentations registered: ${instrumentations.length} total`)
  }

  // Patch global fetch for runtime-agnostic HTTP client tracing (works on Bun, Node.js, Deno)
  const fetchEnabled = config.fetchInstrumentationEnabled ?? DEFAULT_OTEL_CONFIG.fetchInstrumentationEnabled

  if (fetchEnabled) {
    patchGlobalFetch(tracer)
    console.debug('[OTel] Global fetch instrumentation enabled')
  }

  // Initialize logs (always enabled when OTEL is enabled)
  const logExporter = new EngineLogExporter(sharedConnection)
  const logsScheduledDelayMillis =
    config.logsFlushIntervalMs ??
    parseNumberEnv(process.env.OTEL_LOGS_FLUSH_INTERVAL_MS, 0) ??
    DEFAULT_OTEL_CONFIG.logsFlushIntervalMs
  const logsMaxExportBatchSize =
    config.logsBatchSize ?? parseIntegerEnv(process.env.OTEL_LOGS_BATCH_SIZE, 1) ?? DEFAULT_OTEL_CONFIG.logsBatchSize

  loggerProvider = new LoggerProvider({ resource })
  loggerProvider.addLogRecordProcessor(
    new BatchLogRecordProcessor(logExporter, {
      scheduledDelayMillis: logsScheduledDelayMillis,
      maxExportBatchSize: logsMaxExportBatchSize,
    }),
  )
  logger = loggerProvider.getLogger(serviceName)

  console.debug(`[OTel] Logs initialized: delay=${logsScheduledDelayMillis}ms, batch=${logsMaxExportBatchSize}`)
}

/**
 * Shutdown OpenTelemetry, flushing any pending data.
 */
export async function shutdownOtel(): Promise<void> {
  if (tracerProvider) {
    await tracerProvider.forceFlush()
    await tracerProvider.shutdown()
    tracerProvider = null
  }

  if (meterProvider) {
    await meterProvider.forceFlush()
    await meterProvider.shutdown()
    meterProvider = null
  }

  if (loggerProvider) {
    await loggerProvider.forceFlush()
    await loggerProvider.shutdown()
    loggerProvider = null
  }

  if (sharedConnection) {
    await sharedConnection.shutdown()
    sharedConnection = null
  }

  unpatchGlobalFetch()

  tracer = null
  meter = null
  logger = null
}

/**
 * Get the OpenTelemetry tracer instance.
 */
export function getTracer(): Tracer | null {
  return tracer
}

/**
 * Get the OpenTelemetry meter instance.
 */
export function getMeter(): Meter | null {
  return meter
}

/**
 * Get the OpenTelemetry logger instance.
 */
export function getLogger(): Logger | null {
  return logger
}

/**
 * Start a new span with the given name and run the callback within it.
 */
export async function withSpan<T>(
  name: string,
  options: { kind?: SpanKind; traceparent?: string },
  fn: (span: Span) => Promise<T>,
): Promise<T> {
  if (!tracer) {
    // Execute without span context when tracer is not initialized
    // Provide a no-op span to avoid runtime errors if fn calls span methods
    const noopSpan: Span = {
      spanContext: () => ({ traceId: '', spanId: '', traceFlags: 0 }),
      setAttribute: () => noopSpan,
      setAttributes: () => noopSpan,
      addEvent: () => noopSpan,
      addLink: () => noopSpan,
      setStatus: () => noopSpan,
      updateName: () => noopSpan,
      end: () => {},
      isRecording: () => false,
      recordException: () => {},
      addLinks: () => noopSpan,
    }
    return fn(noopSpan)
  }

  const parentContext = options.traceparent ? extractTraceparent(options.traceparent) : context.active()

  return tracer.startActiveSpan(name, { kind: options.kind ?? SpanKind.INTERNAL }, parentContext, async (span) => {
    try {
      const result = await fn(span)
      span.setStatus({ code: SpanStatusCode.OK })
      return result
    } catch (error) {
      span.setStatus({ code: SpanStatusCode.ERROR, message: (error as Error).message })
      span.recordException(error as Error)
      throw error
    } finally {
      span.end()
    }
  })
}

// Re-export OTEL types for convenience
export { SpanKind, SpanStatusCode, SeverityNumber, type Span, type Context, type Tracer, type Meter, type Logger }
