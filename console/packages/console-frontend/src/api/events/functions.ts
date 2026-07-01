import { getDevtoolsApi, getManagementApi } from '../config'
import { unwrapResponse } from '../utils'

// ============================================================================
// Types
// ============================================================================

export interface FunctionInfo {
  function_id: string
  description: string | null
  metadata: Record<string, unknown> | null
  request_format: unknown | null
  response_format: unknown | null
  internal?: boolean
}

// `engine::functions::info` (via `_console/functions/:function_id`) is the only
// surface that carries the request/response schemas — the list route returns
// slim summaries without them.
export interface FunctionDetail {
  function_id: string
  worker_name: string
  description: string | null
  request_schema: unknown | null
  response_schema: unknown | null
  metadata: Record<string, unknown> | null
  registered_triggers: unknown[]
}

export interface TriggerInfo {
  id: string
  trigger_type: string
  function_id: string
  // `config` may be absent on older / leaner engine summaries — always
  // narrow with `?? {}` before reading nested fields. See triggers.tsx
  // and flows.ts for the pattern.
  config?: Record<string, unknown>
  internal?: boolean
}

export interface TriggerTypeInfo {
  id: string
  description: string
}

export interface EventsInfo {
  topic: string
  stream: string
  description: string
}

// ============================================================================
// Functions
// ============================================================================

export async function fetchFunctions(options?: {
  include_internal?: boolean
}): Promise<{ functions: FunctionInfo[]; count: number }> {
  const params = new URLSearchParams()
  if (options?.include_internal) params.set('include_internal', 'true')
  const qs = params.toString()
  const res = await fetch(`${getDevtoolsApi()}/functions${qs ? `?${qs}` : ''}`)
  if (!res.ok) throw new Error('Failed to fetch functions')
  const data = await unwrapResponse<{ functions: FunctionInfo[] }>(res)
  return {
    functions: data.functions || [],
    count: (data.functions || []).length,
  }
}

export async function fetchFunctionDetail(functionId: string): Promise<FunctionDetail> {
  // Function ids are path-segment-safe (`::`-delimited, no slashes) and the
  // engine's http trigger router does not percent-decode path params, so the
  // id is interpolated raw rather than URL-encoded.
  const res = await fetch(`${getDevtoolsApi()}/functions/${functionId}`)
  if (!res.ok) throw new Error('Failed to fetch function detail')
  return unwrapResponse<FunctionDetail>(res)
}

export async function fetchTriggers(options?: {
  include_internal?: boolean
}): Promise<{ triggers: TriggerInfo[]; count: number }> {
  const params = new URLSearchParams()
  if (options?.include_internal) params.set('include_internal', 'true')
  const qs = params.toString()
  try {
    const res = await fetch(`${getDevtoolsApi()}/triggers${qs ? `?${qs}` : ''}`)
    if (res.ok) {
      const data = await unwrapResponse<{ triggers: TriggerInfo[] }>(res)
      return {
        triggers: data.triggers || [],
        count: (data.triggers || []).length,
      }
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/triggers${qs ? `?${qs}` : ''}`)
  if (!res.ok) throw new Error('Failed to fetch triggers')
  const data = await res.json()
  return {
    triggers: data.triggers || [],
    count: (data.triggers || []).length,
  }
}

export async function fetchTriggerTypes(): Promise<{
  trigger_types: string[]
  count: number
}> {
  const res = await fetch(`${getDevtoolsApi()}/trigger-types`)
  if (!res.ok) throw new Error('Failed to fetch trigger types')
  const data = await unwrapResponse<{ trigger_types: string[] }>(res)
  return {
    trigger_types: data.trigger_types || [],
    count: (data.trigger_types || []).length,
  }
}

export async function fetchEventsInfo(): Promise<EventsInfo> {
  const res = await fetch(`${getDevtoolsApi()}/events`)
  if (!res.ok) throw new Error('Failed to fetch events info')
  return unwrapResponse(res)
}
