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
import { registerInstrumentations, type Instrumentation } from '@opentelemetry/instrumentation'
import { LoggerProvider, SimpleLogRecordProcessor } from '@opentelemetry/sdk-logs'
import { type Logger, SeverityNumber } from '@opentelemetry/api-logs'
import { RuntimeNodeInstrumentation } from '@opentelemetry/instrumentation-runtime-node'
import { HostMetrics } from '@opentelemetry/host-metrics'

import {
  type OtelConfig,
  ATTR_SERVICE_VERSION,
  ATTR_SERVICE_NAMESPACE,
  ATTR_SERVICE_INSTANCE_ID,
} from './types'
import { SharedEngineConnection } from './connection'
import { EngineSpanExporter, EngineMetricsExporter, EngineLogExporter } from './exporters'
import { extractTraceparent } from './context'

export * from './types'
export * from './context'

let sharedConnection: SharedEngineConnection | null = null
let tracerProvider: NodeTracerProvider | null = null
let meterProvider: MeterProvider | null = null
let loggerProvider: LoggerProvider | null = null
let tracer: Tracer | null = null
let meter: Meter | null = null
let logger: Logger | null = null
let serviceName: string = 'iii-node-bridge'
let hostMetrics: HostMetrics | null = null

export function initOtel(config: OtelConfig = {}): void {
  const enabled = config.enabled ?? (process.env.OTEL_ENABLED === 'true' || process.env.OTEL_ENABLED === '1')

  if (!enabled) {
    console.log('[OTel] OpenTelemetry is disabled')
    return
  }

  if (config.instrumentations?.length) {
    registerInstrumentations({ instrumentations: config.instrumentations })
  }

  serviceName = config.serviceName ?? process.env.OTEL_SERVICE_NAME ?? 'iii-node-bridge'
  const serviceVersion = config.serviceVersion ?? process.env.SERVICE_VERSION ?? 'unknown'
  const serviceNamespace = config.serviceNamespace ?? process.env.SERVICE_NAMESPACE
  const serviceInstanceId = config.serviceInstanceId ?? process.env.SERVICE_INSTANCE_ID ?? randomUUID()
  const engineWsUrl = config.engineWsUrl ?? process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'

  const resourceAttributes: Record<string, string> = {
    [ATTR_SERVICE_NAME]: serviceName,
    [ATTR_SERVICE_VERSION]: serviceVersion,
    [ATTR_SERVICE_INSTANCE_ID]: serviceInstanceId,
  }
  if (serviceNamespace) {
    resourceAttributes[ATTR_SERVICE_NAMESPACE] = serviceNamespace
  }
  const resource = new Resource(resourceAttributes)

  sharedConnection = new SharedEngineConnection(engineWsUrl, config.reconnectionConfig)

  const spanExporter = new EngineSpanExporter(sharedConnection)
  tracerProvider = new NodeTracerProvider({
    resource,
    spanProcessors: [new SimpleSpanProcessor(spanExporter)],
  })

  propagation.setGlobalPropagator(
    new CompositePropagator({
      propagators: [new W3CTraceContextPropagator(), new W3CBaggagePropagator()],
    })
  )

  tracerProvider.register()
  tracer = trace.getTracer(serviceName)

  console.log(`[OTel] Traces initialized: engine=${engineWsUrl}, service=${serviceName}`)

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

    const runtimeInstrumentation = new RuntimeNodeInstrumentation()
    registerInstrumentations({ instrumentations: [runtimeInstrumentation as unknown as Instrumentation] })

    hostMetrics = new HostMetrics({ meterProvider })
    hostMetrics.start()

    console.log(`[OTel] Metrics initialized: interval=${exportIntervalMs}ms (with runtime & host metrics)`)
  }

  const logExporter = new EngineLogExporter(sharedConnection)
  loggerProvider = new LoggerProvider({ resource })
  loggerProvider.addLogRecordProcessor(new SimpleLogRecordProcessor(logExporter))
  logger = loggerProvider.getLogger(serviceName)

  console.log('[OTel] Logs initialized')
}

export async function shutdownOtel(): Promise<void> {
  if (hostMetrics) {
    hostMetrics = null
  }

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

export function getTracer(): Tracer | null {
  return tracer
}

export function getMeter(): Meter | null {
  return meter
}

export function getLogger(): Logger | null {
  return logger
}

export async function withSpan<T>(
  name: string,
  options: { kind?: SpanKind; traceparent?: string },
  fn: (span: Span) => Promise<T>
): Promise<T> {
  if (!tracer) {
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

export { SpanKind, SpanStatusCode, SeverityNumber, type Span, type Context, type Tracer, type Meter, type Logger }
