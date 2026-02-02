/**
 * OpenTelemetry initialization for the III Node SDK.
 *
 * This module provides trace, metrics, and log export to the III Engine
 * via a shared WebSocket connection using OTLP JSON format.
 */

import { Resource } from '@opentelemetry/resources'
import { ATTR_SERVICE_NAME } from '@opentelemetry/semantic-conventions'
import { randomUUID } from 'crypto'
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
import { SimpleSpanProcessor } from '@opentelemetry/sdk-trace-base'
import { MeterProvider, PeriodicExportingMetricReader } from '@opentelemetry/sdk-metrics'
import { CompositePropagator, W3CBaggagePropagator, W3CTraceContextPropagator } from '@opentelemetry/core'
import { NodeTracerProvider } from '@opentelemetry/sdk-trace-node'
import { registerInstrumentations } from '@opentelemetry/instrumentation'
import { LoggerProvider, SimpleLogRecordProcessor } from '@opentelemetry/sdk-logs'
import { type Logger, SeverityNumber } from '@opentelemetry/api-logs'

import {
  type OtelConfig,
  ATTR_SERVICE_VERSION,
  ATTR_SERVICE_NAMESPACE,
  ATTR_SERVICE_INSTANCE_ID,
} from './types'
import { SharedEngineConnection } from './connection'
import { EngineSpanExporter, EngineMetricsExporter, EngineLogExporter } from './exporters'
import { extractTraceparent } from './context'

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
let serviceName: string = 'iii-node-bridge'

/**
 * Initialize OpenTelemetry with the given configuration.
 * This should be called once at application startup.
 */
export function initOtel(config: OtelConfig = {}): void {
  const enabled = config.enabled ?? (process.env.OTEL_ENABLED === 'true' || process.env.OTEL_ENABLED === '1')

  if (!enabled) {
    console.log('[OTel] OpenTelemetry is disabled')
    return
  }

  // Register any provided instrumentations
  if (config.instrumentations?.length) {
    registerInstrumentations({ instrumentations: config.instrumentations })
  }

  // Configure service identity
  serviceName = config.serviceName ?? process.env.OTEL_SERVICE_NAME ?? 'iii-node-bridge'
  const serviceVersion = config.serviceVersion ?? process.env.SERVICE_VERSION ?? 'unknown'
  const serviceNamespace = config.serviceNamespace ?? process.env.SERVICE_NAMESPACE
  const serviceInstanceId = config.serviceInstanceId ?? process.env.SERVICE_INSTANCE_ID ?? randomUUID()
  const engineWsUrl = config.engineWsUrl ?? process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'

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
    spanProcessors: [new SimpleSpanProcessor(spanExporter)],
  })

  // Register W3C Trace Context and Baggage propagators
  propagation.setGlobalPropagator(
    new CompositePropagator({
      propagators: [new W3CTraceContextPropagator(), new W3CBaggagePropagator()],
    })
  )

  tracerProvider.register()
  tracer = trace.getTracer(serviceName)

  console.log(`[OTel] Traces initialized: engine=${engineWsUrl}, service=${serviceName}`)

  // Initialize metrics if enabled
  const metricsEnabled = config.metricsEnabled ?? (process.env.OTEL_METRICS_ENABLED === 'true' || process.env.OTEL_METRICS_ENABLED === '1')

  if (metricsEnabled) {
    const metricsExporter = new EngineMetricsExporter(sharedConnection)
    const exportIntervalMs = config.metricsExportIntervalMs ?? 60000

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

    console.log(`[OTel] Metrics initialized: interval=${exportIntervalMs}ms`)
  }

  // Initialize logs (always enabled when OTEL is enabled)
  const logExporter = new EngineLogExporter(sharedConnection)
  loggerProvider = new LoggerProvider({ resource })
  loggerProvider.addLogRecordProcessor(new SimpleLogRecordProcessor(logExporter))
  logger = loggerProvider.getLogger(serviceName)

  console.log('[OTel] Logs initialized')
}

/**
 * Shutdown OpenTelemetry, flushing any pending data.
 */
export async function shutdownOtel(): Promise<void> {
  if (tracerProvider) {
    await tracerProvider.shutdown()
    tracerProvider = null
  }

  if (meterProvider) {
    await meterProvider.shutdown()
    meterProvider = null
  }

  if (loggerProvider) {
    await loggerProvider.shutdown()
    loggerProvider = null
  }

  if (sharedConnection) {
    await sharedConnection.shutdown()
    sharedConnection = null
  }

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
  fn: (span: Span) => Promise<T>
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

  return tracer.startActiveSpan(
    name,
    { kind: options.kind ?? SpanKind.INTERNAL },
    parentContext,
    async (span) => {
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
    }
  )
}

// Re-export OTEL types for convenience
export { SpanKind, SpanStatusCode, SeverityNumber, type Span, type Context, type Tracer, type Meter, type Logger }
