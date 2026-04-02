import { getDevtoolsApi } from '../config'
import { unwrapResponse } from '../utils'

export interface WorkerMetrics {
  cpu_percent: number
  cpu_system_micros: number
  cpu_user_micros: number
  event_loop_lag_ms: number
  memory_external: number
  memory_heap_total: number
  memory_heap_used: number
  memory_rss: number
  runtime: string
  timestamp_ms: number
  uptime_seconds: number
}

export interface WorkerInfo {
  id: string
  name: string | null
  runtime: string | null
  version: string | null
  os: string | null
  ip_address: string
  status: string
  connected_at_ms: number
  function_count: number
  functions: string[]
  active_invocations: number
  latest_metrics: WorkerMetrics | null
  internal?: boolean
}

export async function fetchWorkers(): Promise<{
  workers: WorkerInfo[]
  count: number
  timestamp: number
}> {
  const res = await fetch(`${getDevtoolsApi()}/workers`)
  if (!res.ok) throw new Error('Failed to fetch workers')
  const data = await unwrapResponse<{ workers: WorkerInfo[]; timestamp: number }>(res)
  return {
    workers: data.workers || [],
    count: (data.workers || []).length,
    timestamp: data.timestamp,
  }
}
