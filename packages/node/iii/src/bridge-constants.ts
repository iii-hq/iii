/**
 * Constants for the Bridge module.
 */

/** Engine function paths for internal operations */
export const EngineFunctions = {
  LIST_FUNCTIONS: 'engine.functions.list',
  LIST_WORKERS: 'engine.workers.list',
  REGISTER_WORKER: 'engine.workers.register',
} as const

/** Engine trigger types */
export const EngineTriggers = {
  FUNCTIONS_AVAILABLE: 'engine::functions-available',
} as const

/** Log function paths */
export const LogFunctions = {
  INFO: 'log.info',
  WARN: 'log.warn',
  ERROR: 'log.error',
  DEBUG: 'log.debug',
} as const

/** Connection state for the Bridge WebSocket */
export type BridgeConnectionState = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'failed'

/** Configuration for WebSocket reconnection behavior */
export interface BridgeReconnectionConfig {
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
export const DEFAULT_BRIDGE_RECONNECTION_CONFIG: BridgeReconnectionConfig = {
  initialDelayMs: 1000,
  maxDelayMs: 30000,
  backoffMultiplier: 2,
  jitterFactor: 0.3,
  maxRetries: -1,
}

/** Default invocation timeout in milliseconds */
export const DEFAULT_INVOCATION_TIMEOUT_MS = 30000
