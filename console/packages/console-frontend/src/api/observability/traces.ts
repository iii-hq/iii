import { getDevtoolsApi, getManagementApi } from '../config'
import { unwrapResponse } from '../utils'

export interface SpanEvent {
  name: string
  timestamp_unix_nano: number
  attributes: Record<string, unknown>
}

export interface SpanLink {
  trace_id: string
  span_id: string
  attributes: Record<string, unknown>
}

export interface StoredSpan {
  trace_id: string
  span_id: string
  parent_span_id?: string
  name: string
  kind?: string
  start_time_unix_nano: number
  end_time_unix_nano: number
  status: string
  attributes: Array<[string, unknown]>
  events: SpanEvent[]
  links: SpanLink[]
  flags?: number
  service_name?: string
  resource?: Record<string, unknown>
}

export interface TracesResponse {
  spans: StoredSpan[]
  total: number
  offset: number
  limit: number
}

export interface TracesFilterParams {
  trace_id?: string
  service_name?: string
  name?: string
  status?: 'ok' | 'error' | 'unset'
  span_id?: string
  parent_span_id?: string | null
  min_duration_ms?: number
  max_duration_ms?: number
  start_time?: number
  end_time?: number
  attributes?: [string, string][]
  sort_by?: 'start_time' | 'duration' | 'service_name'
  sort_order?: 'asc' | 'desc'
  offset?: number
  limit?: number
  include_internal?: boolean
  search_all_spans?: boolean
}

// Tree API types (engine.traces.tree)
export interface SpanTreeNode {
  trace_id: string
  span_id: string
  parent_span_id?: string
  name: string
  kind?: string
  start_time_unix_nano: number
  end_time_unix_nano: number
  status: string
  attributes: Array<[string, unknown]>
  events: SpanEvent[]
  links: SpanLink[]
  flags?: number
  service_name?: string
  resource?: Record<string, unknown>
  children: SpanTreeNode[]
}

export interface TraceTreeResponse {
  roots: SpanTreeNode[]
}

async function postWithFallback<T>(path: string, body: unknown, errorMsg: string): Promise<T> {
  const init: RequestInit = {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }

  const primaryBase = getDevtoolsApi()
  const fallbackBase = getManagementApi()
  const hasFallback = primaryBase !== fallbackBase

  try {
    const res = await fetch(`${primaryBase}${path}`, init)
    if (res.ok) return unwrapResponse<T>(res)
    if (!hasFallback) throw new Error(errorMsg)
    console.warn(`[traces] Primary API returned ${res.status} for ${path}, trying fallback`)
  } catch (err) {
    if (!hasFallback) throw err instanceof Error ? err : new Error(errorMsg)
    console.warn(`[traces] Primary API failed for ${path}, falling through to fallback`, err)
  }

  const res = await fetch(`${fallbackBase}${path}`, init)
  if (!res.ok) throw new Error(errorMsg)
  return unwrapResponse<T>(res)
}

function stripUndefined(obj: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(obj).filter(([, v]) => v !== undefined))
}

export async function fetchTraces(options?: TracesFilterParams): Promise<TracesResponse> {
  const body = stripUndefined({
    ...options,
    offset: options?.offset ?? 0,
    limit: options?.limit ?? 100,
  })

  return postWithFallback<TracesResponse>('/otel/traces', body, 'Failed to fetch traces')
}

export async function fetchTraceTree(traceId: string): Promise<TraceTreeResponse> {
  return postWithFallback<TraceTreeResponse>(
    '/otel/traces/tree',
    { trace_id: traceId },
    'Failed to fetch trace tree',
  )
}

export async function clearTraces(): Promise<{ success: boolean }> {
  await postWithFallback('/otel/traces/clear', {}, 'Failed to clear traces')
  return { success: true }
}
