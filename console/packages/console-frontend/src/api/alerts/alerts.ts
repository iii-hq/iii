import { getDevtoolsApi, getManagementApi } from '../config'
import { unwrapResponse } from '../utils'

// ============================================================================
// Alert Types (engine.alerts.*)
// ============================================================================

export interface AlertState {
  name: string
  state: 'firing' | 'pending' | 'resolved'
  severity: 'critical' | 'warning' | 'info'
  message: string
  started_at?: number
  resolved_at?: number
  labels: Record<string, string>
  annotations: Record<string, string>
}

export interface AlertsResponse {
  alerts: AlertState[]
  firing_count: number
  timestamp: number
}

// ============================================================================
// Alert Functions (engine.alerts.*)
// ============================================================================

export async function fetchAlerts(): Promise<AlertsResponse> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/alerts`)
    if (res.ok) {
      return unwrapResponse<AlertsResponse>(res)
    }
  } catch {
    // Fall through to management API
  }

  const res = await fetch(`${getManagementApi()}/alerts`)
  if (!res.ok) throw new Error('Failed to fetch alerts')
  return unwrapResponse<AlertsResponse>(res)
}
