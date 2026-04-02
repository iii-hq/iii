import { getConfig, getDevtoolsApi } from '../config'
import { unwrapResponse } from '../utils'

// ============================================================================
// Stream Types
// ============================================================================

export interface StreamInfo {
  id: string
  type: string
  description: string
  groups: string[]
  status: string
  internal?: boolean
}

// ============================================================================
// Stream Functions (used functions only)
// ============================================================================

export async function fetchStreams(): Promise<{
  streams: StreamInfo[]
  count: number
  websocket_port: number
}> {
  console.log('[Streams API] Starting fetch from:', `${getDevtoolsApi()}/streams/list`)
  try {
    const res = await fetch(`${getDevtoolsApi()}/streams/list`, {
      method: 'GET',
    })

    if (!res.ok) {
      throw new Error(`Failed to fetch streams: ${res.statusText}`)
    }

    const data = await unwrapResponse(res)
    return data as {
      streams: StreamInfo[]
      count: number
      websocket_port: number
    }
  } catch (error) {
    console.error('[Streams API] ERROR:', error)
    // Fallback to empty list on error
    return {
      streams: [],
      count: 0,
      websocket_port: getConfig().wsPort,
    }
  }
}
