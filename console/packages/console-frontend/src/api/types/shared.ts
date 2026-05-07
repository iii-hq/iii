/**
 * Shared types used across multiple API modules
 * Extracted from client.ts for cross-domain reuse
 */

export type StreamUpdateOp =
  | { type: 'set'; path: string; value: unknown }
  | { type: 'increment'; path: string; by: number }
  | { type: 'decrement'; path: string; by: number }
  | { type: 'remove'; path: string }
  // Merge accepts a single string (legacy / first-level field) or an
  // array of literal segments (nested path). Path may be omitted to
  // target the root value.
  | { type: 'merge'; path?: string | string[]; value: unknown }
  // Append (issue #1552) mirrors merge's path shape — single string
  // for first-level keys, array for nested paths, omitted for root.
  // Per-op errors include `append.path.*` (validation), `append.target_not_object`,
  // and `append.type_mismatch` (case-2: existing object/scalar at the leaf).
  | { type: 'append'; path?: string | string[]; value: unknown }

export type StreamUpdateOpError = {
  op_index: number
  code: string
  message: string
  doc_url?: string
}

export type StreamUpdateResult = {
  old_value?: unknown
  new_value: unknown
  // Per-op errors. Emitted by `merge` and `append` for validation
  // rejections (depth/size/proto-pollution) and by `append` for the
  // case-2 `append.type_mismatch` and `append.target_not_object`
  // surfaces. Field omitted when empty.
  errors?: StreamUpdateOpError[]
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
