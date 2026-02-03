/**
 * Types, interfaces, and constants for the OpenTelemetry module.
 */

import type { Instrumentation } from '@opentelemetry/instrumentation'

// Semantic convention constants for compatibility across versions
export const ATTR_SERVICE_VERSION = 'service.version'
export const ATTR_SERVICE_NAMESPACE = 'service.namespace'
export const ATTR_SERVICE_INSTANCE_ID = 'service.instance.id'

/** Magic prefixes for binary frames over WebSocket */
export const PREFIX_TRACES = 'OTLP'
export const PREFIX_METRICS = 'MTRC'
export const PREFIX_LOGS = 'LOGS'

/** Connection state for the shared WebSocket */
export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'failed'

/** Configuration for WebSocket reconnection behavior */
export interface ReconnectionConfig {
  /** Starting delay in milliseconds (default: 1000ms) */
  initialDelayMs: number
  /** Maximum delay cap in milliseconds (default: 30000ms) */
  maxDelayMs: number
  /** Exponential backoff multiplier (default: 2) */
  backoffMultiplier: number
  /** Random jitter factor 0-1 (default: 0.3) */
  jitterFactor: number
  /** Maximum retry attempts, -1 for infinite (default: -1) */
  maxRetries: number
}

/** Default reconnection configuration */
export const DEFAULT_RECONNECTION_CONFIG: ReconnectionConfig = {
  initialDelayMs: 1000,
  maxDelayMs: 30000,
  backoffMultiplier: 2,
  jitterFactor: 0.3,
  maxRetries: -1,
}

/** Configuration for OpenTelemetry initialization. */
export interface OtelConfig {
  /** Whether OpenTelemetry export is enabled. Defaults to OTEL_ENABLED env var. */
  enabled?: boolean
  /** The service name to report. Defaults to OTEL_SERVICE_NAME or "iii-node-bridge". */
  serviceName?: string
  /** The service version to report. Defaults to SERVICE_VERSION env var or "unknown". */
  serviceVersion?: string
  /** The service namespace to report. Defaults to SERVICE_NAMESPACE env var. */
  serviceNamespace?: string
  /** The service instance ID to report. Defaults to SERVICE_INSTANCE_ID env var or auto-generated UUID. */
  serviceInstanceId?: string
  /** III Engine WebSocket URL. Defaults to III_BRIDGE_URL or "ws://localhost:49134". */
  engineWsUrl?: string
  /** OpenTelemetry instrumentations to register (e.g., PrismaInstrumentation). */
  instrumentations?: Instrumentation[]
  /** Whether OpenTelemetry metrics export is enabled. Defaults to OTEL_METRICS_ENABLED env var. */
  metricsEnabled?: boolean
  /** Metrics export interval in milliseconds. Defaults to 60000 (60 seconds). */
  metricsExportIntervalMs?: number
  /** Optional reconnection configuration for the WebSocket connection. */
  reconnectionConfig?: Partial<ReconnectionConfig>
}
