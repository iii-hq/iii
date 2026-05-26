export { Logger } from './logger'
export { executeTracedRequest } from './http-instrumentation'
export type { TracedFetchInit } from './http-instrumentation'
export {
  BaggageSpanProcessor,
  DEFAULT_ALLOWLIST,
  currentSpanId,
  currentSpanIsRecording,
  currentTraceId,
  extractBaggage,
  extractContext,
  extractTraceparent,
  flushOtel,
  getAllBaggage,
  getBaggageEntry,
  getLogger,
  getMeter,
  getTracer,
  initOtel,
  injectBaggage,
  injectTraceparent,
  recordSpanEvent,
  redact,
  redactAndTruncate,
  REDACTED_PLACEHOLDER,
  removeBaggageEntry,
  resolveMaxBytesFromEnv,
  setBaggageEntry,
  setCurrentSpanAttribute,
  setCurrentSpanError,
  SeverityNumber,
  shutdownOtel,
  SpanKind,
  withSpan,
} from './telemetry-system'
export type {
  OtelApiLogger as OtelLogger,
  Meter,
  OtelConfig,
  ReconnectionConfig,
  Span,
} from './telemetry-system'
export { patchGlobalFetch, unpatchGlobalFetch } from './telemetry-system/fetch-instrumentation'
export { registerWorkerGauges, stopWorkerGauges } from './otel-worker-gauges'
export type { WorkerGaugesOptions } from './otel-worker-gauges'
export { WorkerMetricsCollector } from './worker-metrics'
export type { WorkerMetrics, WorkerMetricsCollectorOptions } from './worker-metrics'
export type { OtelLogEvent } from './types'
export { safeStringify } from './utils'
