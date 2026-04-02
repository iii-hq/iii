/**
 * Shared types used across multiple API modules
 * Extracted from client.ts for cross-domain reuse
 */

export type StreamUpdateOp =
  | { type: 'set'; path: string; value: unknown }
  | { type: 'increment'; path: string; by: number }
  | { type: 'decrement'; path: string; by: number }
  | { type: 'remove'; path: string }
  | { type: 'merge'; path: string; value: unknown }

export type StreamUpdateResult = {
  old_value?: unknown
  new_value: unknown
}

export type MetricsSnapshot = {
  id?: string
  timestamp: number
  functions_count: number
  triggers_count: number
  workers_count: number
  uptime_seconds: number
}

export type StreamMessage = {
  timestamp: number
  streamName: string
  groupId: string
  id: string | null
  event: {
    type: 'sync' | 'create' | 'update' | 'delete' | 'unauthorized'
    data?: unknown
  }
}
