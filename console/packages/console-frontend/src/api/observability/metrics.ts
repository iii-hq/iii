import { getDevtoolsApi, getManagementApi } from '../config'
import type { MetricsSnapshot } from '../types/shared'
import { unwrapResponse } from '../utils'

// ============================================================================
// Metrics Types (engine.metrics.*)
// ============================================================================

export interface InvocationMetrics {
  total: number
  success: number
  error: number
  deferred: number
  by_function: Record<string, number>
}

export interface WorkerPoolMetrics {
  spawns: number
  deaths: number
  active: number
}

export interface PerformanceMetrics {
  avg_duration_ms: number
  p50_duration_ms: number
  p95_duration_ms: number
  p99_duration_ms: number
  min_duration_ms: number
  max_duration_ms: number
}

export interface SdkMetric {
  name: string
  description?: string
  unit?: string
  value: number | Record<string, number>
  type: 'counter' | 'gauge' | 'histogram'
  timestamp: number
}

export interface DetailedMetricsResponse {
  engine_metrics: {
    invocations: InvocationMetrics
    workers: WorkerPoolMetrics
    performance: PerformanceMetrics
  }
  sdk_metrics: SdkMetric[]
  timestamp: number
}

// ============================================================================
// Rollup Types (engine.rollups.*)
// ============================================================================

export interface Rollup {
  metric_name: string
  timestamp: number
  count: number
  sum: number
  min: number
  max: number
  avg: number
}

export interface HistogramRollup {
  metric_name: string
  timestamp: number
  count: number
  sum: number
  buckets: Record<string, number>
}

export interface RollupsResponse {
  rollups: Rollup[]
  histogram_rollups: HistogramRollup[]
  level: number
  timestamp: number
}

export async function fetchMetrics(): Promise<MetricsSnapshot> {
  const res = await fetch(`${getDevtoolsApi()}/metrics`)
  if (!res.ok) throw new Error('Failed to fetch metrics')
  return unwrapResponse(res)
}

export async function fetchMetricsHistory(
  limit?: number,
): Promise<{ history: MetricsSnapshot[]; count: number }> {
  const url = limit
    ? `${getDevtoolsApi()}/metrics/history?limit=${limit}`
    : `${getDevtoolsApi()}/metrics/history`
  const res = await fetch(url)
  if (!res.ok) throw new Error('Failed to fetch metrics history')
  return unwrapResponse(res)
}

export async function fetchDetailedMetrics(options?: {
  start_time?: number
  end_time?: number
  metric_name?: string
  aggregate_interval?: number
}): Promise<DetailedMetricsResponse> {
  const body = {
    start_time: options?.start_time,
    end_time: options?.end_time,
    metric_name: options?.metric_name,
    aggregate_interval: options?.aggregate_interval,
  }

  try {
    const res = await fetch(`${getDevtoolsApi()}/metrics/detailed`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (res.ok) {
      return unwrapResponse<DetailedMetricsResponse>(res)
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/metrics/detailed`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error('Failed to fetch detailed metrics')
  return res.json()
}

export async function fetchRollups(options?: {
  start_time?: number
  end_time?: number
  level?: number // 0=1min, 1=5min, 2=1hour
  metric_name?: string
}): Promise<RollupsResponse> {
  const body = {
    start_time: options?.start_time,
    end_time: options?.end_time,
    level: options?.level,
    metric_name: options?.metric_name,
  }

  try {
    const res = await fetch(`${getManagementApi()}/rollups`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (res.ok) {
      return unwrapResponse<RollupsResponse>(res)
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/rollups`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error('Failed to fetch rollups')
  return res.json()
}
