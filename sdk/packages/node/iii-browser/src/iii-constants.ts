/**
 * Constants for the III module.
 */

/**
 * Engine function paths for internal operations.
 *
 * Naming note: `LIST_TRIGGERS` / `INFO_TRIGGERS` cover trigger TYPES
 * (templates). `LIST_REGISTERED_TRIGGERS` / `INFO_REGISTERED_TRIGGERS`
 * cover trigger INSTANCES (subscriber rows). The old
 * `engine::trigger-types::list` builtin has been removed and is now
 * served by `engine::triggers::list`.
 */
export const EngineFunctions = {
  LIST_FUNCTIONS: 'engine::functions::list',
  INFO_FUNCTIONS: 'engine::functions::info',
  LIST_WORKERS: 'engine::workers::list',
  INFO_WORKERS: 'engine::workers::info',
  LIST_TRIGGERS: 'engine::triggers::list',
  INFO_TRIGGERS: 'engine::triggers::info',
  LIST_REGISTERED_TRIGGERS: 'engine::registered-triggers::list',
  INFO_REGISTERED_TRIGGERS: 'engine::registered-triggers::info',
  REGISTER_WORKER: 'engine::workers::register',
} as const

/** Engine trigger types */
export const EngineTriggers = {
  FUNCTIONS_AVAILABLE: 'engine::functions-available',
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
