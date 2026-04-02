import { getDevtoolsApi, getManagementApi } from '../config'
import { unwrapResponse } from '../utils'

// ============================================================================
// Log Types (engine.otel.logs.*)
// ============================================================================

export interface OtelLog {
  timestamp_unix_nano: number
  observed_timestamp_unix_nano: number
  trace_id?: string | null
  span_id?: string | null
  severity_number: number
  severity_text: string
  body: string
  attributes: Record<string, unknown>
  resource: Record<string, unknown>
  instrumentation_scope_name?: string
  instrumentation_scope_version?: string
  service_name?: string
}

export interface OtelLogsResponse {
  logs: OtelLog[]
  total: number
  query: {
    start_time?: number
    end_time?: number
    trace_id?: string
    span_id?: string
    severity_min?: number
    severity_text?: string
    offset: number
    limit: number
  }
  timestamp: number
}

export interface LogEntry {
  id: string
  timestamp: number
  level: string
  message: string
  source: string
  metadata?: Record<string, unknown>
}

export interface LegacyLogEntry {
  trace_id?: string
  date?: string
  level?: string
  message: string
  function_name?: string
  args?: string
}

export interface LogWriteInput {
  message: string
  trace_id?: string
  span_id?: string
  data?: unknown
  service_name?: string
}

export async function fetchOtelLogs(options?: {
  start_time?: number
  end_time?: number
  trace_id?: string
  span_id?: string
  severity_min?: number
  severity_text?: string
  offset?: number
  limit?: number
}): Promise<OtelLogsResponse> {
  const body = {
    start_time: options?.start_time,
    end_time: options?.end_time,
    trace_id: options?.trace_id,
    span_id: options?.span_id,
    severity_min: options?.severity_min,
    severity_text: options?.severity_text,
    offset: options?.offset ?? 0,
    limit: options?.limit ?? 100,
  }

  try {
    const res = await fetch(`${getDevtoolsApi()}/otel/logs`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (res.ok) {
      return unwrapResponse<OtelLogsResponse>(res)
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/otel/logs`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error('Failed to fetch OTEL logs')
  return res.json()
}

export async function clearOtelLogs(): Promise<{ success: boolean }> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/otel/logs/clear`, {
      method: 'POST',
    })
    if (res.ok) {
      await unwrapResponse(res)
      return { success: true }
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/otel/logs/clear`, {
    method: 'POST',
  })
  if (!res.ok) throw new Error('Failed to clear OTEL logs')
  return { success: true }
}

export async function fetchLogs(options?: {
  level?: string
  limit?: number
  since?: number
}): Promise<{
  logs: LegacyLogEntry[]
  count: number
  filter: { level: string; limit: number; since: number }
  info: { message: string; adapters: string[] }
}> {
  const data = await fetchOtelLogs({
    offset: 0,
    limit: options?.limit ?? 500,
    severity_text: options?.level,
    start_time: options?.since,
  })

  const logs = (data.logs || []).map((log) => ({
    trace_id: log.trace_id || undefined,
    date: new Date(log.timestamp_unix_nano / 1_000_000).toISOString(),
    level: log.severity_text?.toLowerCase() || 'info',
    message: log.body,
    function_name:
      (log.attributes?.function_name as string) ||
      (log.resource?.['service.name'] as string) ||
      'unknown',
    args: log.attributes ? JSON.stringify(log.attributes) : undefined,
  }))

  return {
    logs,
    count: logs.length,
    filter: {
      level: options?.level || 'all',
      limit: options?.limit || 500,
      since: options?.since || 0,
    },
    info: {
      message: `Fetched ${logs.length} logs`,
      adapters: [],
    },
  }
}
