import { queryOptions } from '@tanstack/react-query'
import { fetchAlerts } from './alerts/alerts'
import { fetchSamplingRules } from './alerts/sampling'
import {
  fetchEventsInfo,
  fetchFunctions,
  fetchTriggers,
  fetchTriggerTypes,
} from './events/functions'
import { fetchFlowConfig, fetchFlows } from './flows/flows'
import { fetchLogs, fetchOtelLogs } from './observability/logs'
import {
  fetchDetailedMetrics,
  fetchMetrics,
  fetchMetricsHistory,
  fetchRollups,
} from './observability/metrics'
import { fetchTraces, fetchTraceTree } from './observability/traces'
import { fetchDlqMessages, fetchDlqTopics, fetchQueueDetail, fetchQueues } from './queues'
import { fetchStateGroups, fetchStateItems } from './state/state'
import { fetchStreams } from './state/streams'
import { fetchAdapters } from './system/adapters'
import { fetchStatus, healthCheck } from './system/status'
import { fetchWorkers } from './system/workers'

// System status
export const statusQuery = queryOptions({
  queryKey: ['status'],
  queryFn: fetchStatus,
})

// Health check
export const healthQuery = queryOptions({
  queryKey: ['health'],
  queryFn: healthCheck,
  refetchInterval: 5000,
})

// Functions
export const functionsQuery = (options?: { include_internal?: boolean }) =>
  queryOptions({
    queryKey: ['functions', options?.include_internal ?? false],
    queryFn: () => fetchFunctions(options),
  })

// Triggers
export const triggersQuery = (options?: { include_internal?: boolean }) =>
  queryOptions({
    queryKey: ['triggers', options?.include_internal ?? false],
    queryFn: () => fetchTriggers(options),
  })

// Trigger types
export const triggerTypesQuery = queryOptions({
  queryKey: ['trigger-types'],
  queryFn: fetchTriggerTypes,
})

// Workers
export const workersQuery = queryOptions({
  queryKey: ['workers'],
  queryFn: fetchWorkers,
})

// Metrics
export const metricsQuery = queryOptions({
  queryKey: ['metrics'],
  queryFn: fetchMetrics,
})

// Metrics history
export const metricsHistoryQuery = (limit?: number) =>
  queryOptions({
    queryKey: ['metrics-history', limit],
    queryFn: () => fetchMetricsHistory(limit),
  })

// Events info
export const eventsInfoQuery = queryOptions({
  queryKey: ['events-info'],
  queryFn: fetchEventsInfo,
})

// Streams
export const streamsQuery = queryOptions({
  queryKey: ['streams'],
  queryFn: fetchStreams,
})

// Queues
export const queuesQuery = queryOptions({
  queryKey: ['queues'],
  queryFn: fetchQueues,
})

export function queueDetailQuery(topic: string) {
  return queryOptions({
    queryKey: ['queue-detail', topic],
    queryFn: () => fetchQueueDetail(topic),
    enabled: !!topic,
  })
}

// DLQ
export const dlqTopicsQuery = queryOptions({
  queryKey: ['dlq-topics'],
  queryFn: fetchDlqTopics,
})

export function dlqMessagesQuery(topic: string, offset = 0, limit = 50) {
  return queryOptions({
    queryKey: ['dlq-messages', topic, offset, limit],
    queryFn: () => fetchDlqMessages(topic, offset, limit),
    enabled: !!topic,
  })
}

// Logs with filters
export interface LogsQueryOptions {
  level?: string
  limit?: number
  since?: number
}

export const logsQuery = (options?: LogsQueryOptions) =>
  queryOptions({
    queryKey: ['logs', options],
    queryFn: () => fetchLogs(options),
  })

// Adapters
export const adaptersQuery = queryOptions({
  queryKey: ['adapters'],
  queryFn: fetchAdapters,
})

// State groups for a stream
export const stateGroupsQuery = () =>
  queryOptions({
    queryKey: ['state-groups'],
    queryFn: () => fetchStateGroups(),
  })

// State items for a group
export const stateItemsQuery = (groupId: string) =>
  queryOptions({
    queryKey: ['state-items', groupId],
    queryFn: () => fetchStateItems(groupId),
    enabled: !!groupId,
  })

// Alerts
export const alertsQuery = queryOptions({
  queryKey: ['alerts'],
  queryFn: fetchAlerts,
})

// Sampling rules
export const samplingRulesQuery = queryOptions({
  queryKey: ['sampling-rules'],
  queryFn: fetchSamplingRules,
})

// OTEL logs with options
export const otelLogsQuery = (options?: {
  start_time?: number
  end_time?: number
  trace_id?: string
  span_id?: string
  severity_min?: number
  severity_text?: string
  offset?: number
  limit?: number
}) => {
  // Build queryKey with only defined values to ensure proper cache invalidation
  const queryKey: Array<string | Record<string, unknown>> = ['otel-logs']
  const filterParams: Record<string, unknown> = {}

  if (options?.start_time !== undefined) filterParams.start_time = options.start_time
  if (options?.end_time !== undefined) filterParams.end_time = options.end_time
  if (options?.trace_id !== undefined) filterParams.trace_id = options.trace_id
  if (options?.span_id !== undefined) filterParams.span_id = options.span_id
  if (options?.severity_min !== undefined) filterParams.severity_min = options.severity_min
  if (options?.severity_text !== undefined) filterParams.severity_text = options.severity_text
  if (options?.offset !== undefined) filterParams.offset = options.offset
  if (options?.limit !== undefined) filterParams.limit = options.limit

  if (Object.keys(filterParams).length > 0) {
    queryKey.push(filterParams)
  }

  return queryOptions({
    queryKey,
    queryFn: () => fetchOtelLogs(options),
  })
}

// OTEL traces with options
export const otelTracesQuery = (options?: { trace_id?: string; offset?: number; limit?: number }) =>
  queryOptions({
    queryKey: ['otel-traces', options],
    queryFn: () => fetchTraces(options),
  })

// OTEL trace tree (single trace with nested children)
export const otelTraceTreeQuery = (traceId: string) =>
  queryOptions({
    queryKey: ['otel-trace-tree', traceId],
    queryFn: () => fetchTraceTree(traceId),
    enabled: !!traceId,
  })

// Detailed metrics with options
export const detailedMetricsQuery = (options?: {
  start_time?: number
  end_time?: number
  metric_name?: string
  aggregate_interval?: number
}) =>
  queryOptions({
    queryKey: ['detailed-metrics', options],
    queryFn: () => fetchDetailedMetrics(options),
  })

// Rollups with options
export const rollupsQuery = (options?: {
  start_time?: number
  end_time?: number
  metric_name?: string
  aggregate_interval?: number
}) =>
  queryOptions({
    queryKey: ['rollups', options],
    queryFn: () => fetchRollups(options),
  })

// Flows
export const flowsQuery = () =>
  queryOptions({
    queryKey: ['flows'],
    queryFn: fetchFlows,
    refetchInterval: 10000,
  })

// Flow config
export const flowConfigQuery = (flowId: string) =>
  queryOptions({
    queryKey: ['flow-config', flowId],
    queryFn: () => fetchFlowConfig(flowId),
    enabled: !!flowId,
  })
