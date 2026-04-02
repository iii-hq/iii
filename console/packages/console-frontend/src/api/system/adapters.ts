import { getDevtoolsApi } from '../config'
import { unwrapResponse } from '../utils'

export interface AdapterInfo {
  id: string
  type: string
  status: string
  health: string
  description?: string
  count?: number
  port?: number
  internal?: boolean
}

export async function fetchAdapters(): Promise<{ adapters: AdapterInfo[]; count: number }> {
  const res = await fetch(`${getDevtoolsApi()}/adapters`)
  if (!res.ok) throw new Error('Failed to fetch adapters')
  return unwrapResponse(res)
}
