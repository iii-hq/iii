import type { Trace, TraceGroup, TraceGroupMeta } from '../types/observability'

export const deriveTraceGroup = (meta: TraceGroupMeta, traces: Trace[]): TraceGroup => {
  const completedSteps = traces.filter((t) => t.status === 'completed').length
  const failedCount = traces.filter((t) => t.status === 'failed').length
  const runningCount = traces.filter((t) => t.status === 'running').length

  const status = failedCount > 0 ? 'failed' : runningCount > 0 ? 'running' : 'completed'

  const endTimes = traces.filter((t): t is Trace & { endTime: number } => t.endTime !== undefined).map((t) => t.endTime)
  const endTime = runningCount === 0 && endTimes.length > 0 ? Math.max(...endTimes) : undefined

  const lastActivity = Math.max(
    meta.startTime,
    ...traces.map((t) => t.endTime || t.startTime),
    ...traces.flatMap((t) => t.events.map((e) => e.timestamp)),
  )

  return {
    ...meta,
    status,
    endTime,
    lastActivity,
    metadata: {
      completedSteps,
      activeSteps: runningCount,
      totalSteps: traces.length,
    },
  }
}
