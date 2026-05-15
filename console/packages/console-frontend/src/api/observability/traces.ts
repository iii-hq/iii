import type { ISdk } from 'iii-browser-sdk'

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

export interface TracesGroupByParams {
  /** Span attribute key to group by, e.g. "iii.message.id". */
  attribute: string
  /** Earliest end_time (ms since epoch) to include. Omit for no lower bound. */
  since_ms?: number
  /** Max groups returned after sorting by `first_seen_ms` descending. Default 100 server-side. */
  limit?: number
  /** Include engine-internal spans. Defaults to false server-side, matching `traces::list`. */
  include_internal?: boolean
}

export interface TraceGroup {
  /** The attribute value this group is keyed on. */
  value: string
  trace_ids: string[]
  span_count: number
  first_seen_ms: number
  last_seen_ms: number
  duration_ms: number
  error_count: number
}

export interface TracesGroupByResponse {
  groups: TraceGroup[]
}

const FN_LIST = 'engine::traces::list'
const FN_TREE = 'engine::traces::tree'
const FN_CLEAR = 'engine::traces::clear'
const FN_GROUP_BY = 'engine::traces::group_by'

const LIST_TIMEOUT_MS = 5_000
const TREE_TIMEOUT_MS = 10_000
const CLEAR_TIMEOUT_MS = 5_000
const GROUP_BY_TIMEOUT_MS = 5_000

function stripUndefined<T extends Record<string, unknown>>(obj: T): T {
  return Object.fromEntries(Object.entries(obj).filter(([, v]) => v !== undefined)) as T
}

function isMemoryExporterNotEnabled(err: unknown): boolean {
  const msg = err instanceof Error ? err.message : String(err)
  return /memory exporter (is )?not enabled/i.test(msg)
}

function asError(err: unknown, fallback: string): Error {
  if (err instanceof Error) return err
  return new Error(typeof err === 'string' ? err : fallback)
}

export async function fetchTraces(
  sdk: ISdk,
  options?: TracesFilterParams,
): Promise<TracesResponse> {
  const offset = options?.offset ?? 0
  const limit = options?.limit ?? 100
  const payload = stripUndefined({ ...options, offset, limit })

  try {
    return await sdk.trigger<typeof payload, TracesResponse>({
      function_id: FN_LIST,
      payload,
      timeoutMs: LIST_TIMEOUT_MS,
    })
  } catch (err) {
    if (isMemoryExporterNotEnabled(err)) {
      return { spans: [], total: 0, offset, limit }
    }
    throw asError(err, 'Failed to fetch traces')
  }
}

export async function fetchTraceTree(
  sdk: ISdk,
  traceId: string,
): Promise<TraceTreeResponse> {
  try {
    return await sdk.trigger<{ trace_id: string }, TraceTreeResponse>({
      function_id: FN_TREE,
      payload: { trace_id: traceId },
      timeoutMs: TREE_TIMEOUT_MS,
    })
  } catch (err) {
    if (isMemoryExporterNotEnabled(err)) {
      return { roots: [] }
    }
    throw asError(err, 'Failed to fetch trace tree')
  }
}

export async function clearTraces(sdk: ISdk): Promise<{ success: boolean }> {
  try {
    await sdk.trigger<Record<string, never>, { success: boolean }>({
      function_id: FN_CLEAR,
      payload: {},
      timeoutMs: CLEAR_TIMEOUT_MS,
    })
    return { success: true }
  } catch (err) {
    throw asError(err, 'Failed to clear traces')
  }
}

/**
 * Server-side aggregation by attribute value. Calls the engine function
 * `engine::traces::group_by`. When the memory exporter is not enabled,
 * returns an empty group list so the UI degrades to the same "OTel not
 * configured" path as `fetchTraces`. When the engine itself doesn't
 * expose `group_by` (older deploy), the error is rethrown — callers
 * (see `useTraceGroups`) detect it via `isGroupByUnavailable` and hide
 * the group-by affordance.
 */
export async function fetchTracesGroupBy(
  sdk: ISdk,
  params: TracesGroupByParams,
): Promise<TracesGroupByResponse> {
  const payload = stripUndefined({ ...params })
  try {
    return await sdk.trigger<typeof payload, TracesGroupByResponse>({
      function_id: FN_GROUP_BY,
      payload,
      timeoutMs: GROUP_BY_TIMEOUT_MS,
    })
  } catch (err) {
    if (isMemoryExporterNotEnabled(err)) {
      return { groups: [] }
    }
    throw asError(err, 'Failed to fetch trace groups')
  }
}
