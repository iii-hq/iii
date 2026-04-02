import { getDevtoolsApi, getManagementApi } from '../config'
import { unwrapResponse } from '../utils'

export interface SystemStatus {
  status: string
  uptime_seconds: number
  uptime_formatted: string
  workers: number
  functions: number
  triggers: number
  version: string
  timestamp: number
  metrics_available?: boolean
}

export interface HealthComponent {
  status: string
  details: Record<string, unknown>
}

export interface HealthStatus {
  status: string
  timestamp: number
  version: string
  components: {
    logs?: HealthComponent
    metrics?: HealthComponent
    otel?: HealthComponent
    spans?: HealthComponent
  }
}

export async function fetchStatus(): Promise<SystemStatus> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/status`)
    if (res.ok) {
      const data = await unwrapResponse<{
        status: string
        workers: number
        functions: number
        metrics_available: boolean
      }>(res)
      return {
        status: data.status,
        uptime_seconds: 0,
        uptime_formatted: '—',
        workers: data.workers,
        functions: data.functions,
        triggers: 0,
        version: '',
        timestamp: Date.now() / 1000,
        metrics_available: data.metrics_available,
      }
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/status`)
  if (!res.ok) throw new Error('Failed to fetch status')
  const data = await res.json()

  return {
    status: data.status,
    uptime_seconds: 0,
    uptime_formatted: data.uptime || '—',
    workers: data.workers,
    functions: data.functions,
    triggers: 0,
    version: data.version,
    timestamp: Date.now() / 1000,
  }
}

export async function healthCheck(): Promise<HealthStatus> {
  const res = await fetch(`${getDevtoolsApi()}/health`)
  if (!res.ok) throw new Error('Health check failed')
  return unwrapResponse(res)
}

export async function isDevToolsAvailable(): Promise<boolean> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/health`)
    return res.ok
  } catch {
    return false
  }
}

export async function isManagementApiAvailable(): Promise<boolean> {
  try {
    const res = await fetch(`${getManagementApi()}/status`)
    return res.ok
  } catch {
    return false
  }
}

export async function getConnectionStatus(): Promise<{
  devtools: boolean
  management: boolean
}> {
  const [devtools, management] = await Promise.all([
    isDevToolsAvailable(),
    isManagementApiAvailable(),
  ])
  return { devtools, management }
}
