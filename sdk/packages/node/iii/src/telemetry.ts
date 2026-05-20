export {
  DEFAULT_BRIDGE_RECONNECTION_CONFIG,
  DEFAULT_INVOCATION_TIMEOUT_MS,
  EngineFunctions,
  EngineTriggers,
  type IIIConnectionState,
  type IIIReconnectionConfig,
  LogFunctions,
} from './iii-constants'
export type { RegisterFunctionFormat } from './iii-types'
export {
  registerWorkerGauges,
  stopWorkerGauges,
  type WorkerGaugesOptions,
} from './otel-worker-gauges'
export {
  BaggageSpanProcessor,
  currentSpanId,
  currentSpanIsRecording,
  currentTraceId,
  DEFAULT_ALLOWLIST,
  extractBaggage,
  extractContext,
  extractTraceparent,
  getAllBaggage,
  getBaggageEntry,
  getLogger,
  getMeter,
  getTracer,
  initOtel,
  injectBaggage,
  injectTraceparent,
  type Logger as OtelLogger,
  type Meter,
  type OtelConfig,
  type ReconnectionConfig,
  redact,
  redactAndTruncate,
  REDACTED_PLACEHOLDER,
  resolveMaxBytesFromEnv,
  recordSpanEvent,
  removeBaggageEntry,
  SeverityNumber,
  setCurrentSpanAttribute,
  setCurrentSpanError,
  type Span,
  SpanStatusCode,
  setBaggageEntry,
  shutdownOtel,
  withSpan,
} from './telemetry-system'
export type { OtelLogEvent } from './types'
export { safeStringify } from './utils'
export type { WorkerMetrics, WorkerMetricsCollectorOptions } from './worker-metrics'
export { WorkerMetricsCollector } from './worker-metrics'
