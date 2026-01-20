export interface TraceGroupMeta {
  id: string
  name: string
  startTime: number
}

export interface TraceGroup extends TraceGroupMeta {
  status: 'running' | 'completed' | 'failed'
  endTime?: number
  lastActivity: number
  metadata: {
    completedSteps: number
    activeSteps: number
    totalSteps: number
  }
}

export interface Trace {
  id: string
  name: string
  correlationId?: string
  parentTraceId?: string
  status: 'running' | 'completed' | 'failed'
  startTime: number
  endTime?: number
  entryPoint: { type: 'api' | 'event' | 'cron'; stepName: string }
  events: TraceEvent[]
  error?: TraceError
}

export type TraceError = {
  message: string
  code?: string | number
  stack?: string
}

export type TraceEvent = StateEvent | EmitEvent | StreamEvent | LogEntry

export type StateOperation = 'get' | 'getGroup' | 'set' | 'delete' | 'clear'
export type StreamOperation = 'get' | 'getGroup' | 'set' | 'delete' | 'clear' | 'send'

export interface StateEvent {
  type: 'state'
  timestamp: number
  operation: 'get' | 'getGroup' | 'set' | 'delete' | 'clear'
  key?: string
  duration?: number
  data: any
}

export interface EmitEvent {
  type: 'emit'
  timestamp: number
  topic: string
  success: boolean
  data: unknown
}

export interface StreamEvent {
  type: 'stream'
  timestamp: number
  operation: StreamOperation
  streamName: string
  duration?: number
  data: { groupId: string; id: string; data?: unknown }
  calls: number
}

export interface LogEntry {
  type: 'log'
  timestamp: number
  level: string
  message: string
  metadata?: unknown
}

export interface ObservabilityStats {
  total: number
  running: number
  completed: number
  failed: number
}
