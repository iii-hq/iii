/**
 * Constants for the III module.
 */

/** Engine function paths for internal operations. */
export const EngineFunctions = {
  LIST_FUNCTIONS: 'engine::functions::list',
  INFO_FUNCTIONS: 'engine::functions::info',
  LIST_WORKERS: 'engine::workers::list',
  INFO_WORKERS: 'engine::workers::info',
  REGISTER_WORKER: 'engine::workers::register',
} as const

/**
 * The event sources the engine hosts: private functions whose handler
 * returns the event, observed with `iii.observe(EngineSources.X, target)`.
 * None takes args; filter on the event (`input`) instead.
 */
export const EngineSources = {
  /** A function was registered or unregistered; each row carries `observer_count`. */
  FUNCTIONS_CHANGED: 'engine::on-functions-changed',
  /** A worker connected or left. */
  WORKERS_CHANGED: 'engine::on-workers-changed',
  /** An observer changed state: `{ observer_id, function_id, target, state, reason? }`. */
  OBSERVERS_CHANGED: 'engine::on-observers-changed',
  /** A log record. */
  LOG: 'observability::on-log',
  /** A finished span. */
  SPAN: 'observability::on-span',
  /** The engine's configuration was reloaded. */
  CONFIGURATION_UPDATED: 'configuration::on-updated',
  /** A stream item changed; filter on `input.stream_name`, `input.group_id`. */
  STREAM_CHANGED: 'stream::on-changed',
  /** A client joined a stream group. */
  STREAM_JOIN: 'stream::on-join',
  /** A client left a stream group. */
  STREAM_LEAVE: 'stream::on-leave',
} as const

/** Connection state for the III WebSocket */
export type IIIConnectionState = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'failed'

/** Configuration for WebSocket reconnection behavior */
export interface IIIReconnectionConfig {
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
export const DEFAULT_BRIDGE_RECONNECTION_CONFIG: IIIReconnectionConfig = {
  initialDelayMs: 1000,
  maxDelayMs: 30000,
  backoffMultiplier: 2,
  jitterFactor: 0.3,
  maxRetries: -1,
}

/** Default invocation timeout in milliseconds */
export const DEFAULT_INVOCATION_TIMEOUT_MS = 30000
