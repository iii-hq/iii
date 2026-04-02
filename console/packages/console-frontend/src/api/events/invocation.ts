import { getDevtoolsApi } from '../config'

async function extractApiError(res: Response, fallback: string): Promise<string> {
  const contentType = res.headers.get('content-type') || ''

  if (contentType.includes('application/json')) {
    try {
      const data = await res.json()
      if (data && typeof data === 'object') {
        const wrappedBody =
          'status_code' in data &&
          'body' in data &&
          data.body &&
          typeof data.body === 'object' &&
          !Array.isArray(data.body)
            ? (data.body as Record<string, unknown>)
            : null

        if (wrappedBody?.error && typeof wrappedBody.error === 'string') {
          return wrappedBody.error
        }

        if ('error' in data && typeof data.error === 'string') {
          return data.error
        }

        return JSON.stringify(wrappedBody ?? data)
      }
    } catch {
      // Fall back to text below when body is not valid JSON.
    }
  }

  try {
    const text = await res.text()
    return text || fallback
  } catch {
    return fallback
  }
}

export async function invokeFunction(
  functionId: string,
  input?: unknown,
): Promise<{ success: boolean; data?: unknown; error?: string }> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/invoke`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ function_id: functionId, input: input || {} }),
    })

    if (res.ok) {
      const data = await res.json()
      return { success: true, data }
    } else {
      const error = await res.text()
      return { success: false, error: error || 'Invocation failed' }
    }
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : 'Network error' }
  }
}

export async function emitEvent(
  topic: string,
  data: unknown,
): Promise<{ success: boolean; error?: string }> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/emit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ topic, data }),
    })

    if (res.ok) {
      return { success: true }
    } else {
      const error = await res.text()
      if (res.status === 404) {
        return {
          success: false,
          error: 'Event emit endpoint not available. Add /_console/emit to DevTools module.',
        }
      }
      return { success: false, error: error || 'Emit failed' }
    }
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : 'Network error' }
  }
}

export async function triggerCron(
  triggerId: string,
  functionId?: string,
): Promise<{ success: boolean; data?: unknown; error?: string }> {
  try {
    const res = await fetch(`${getDevtoolsApi()}/cron/trigger`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ trigger_id: triggerId, function_id: functionId }),
    })

    if (res.ok) {
      const data = await res.json()
      return { success: true, data }
    } else {
      if (res.status === 404) {
        return {
          success: false,
          error:
            'Cron trigger endpoint is not available in this console backend version. Update iii-console to enable manual cron trigger support.',
        }
      }
      const error = await extractApiError(res, 'Trigger failed')
      return { success: false, error: error || 'Trigger failed' }
    }
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : 'Network error' }
  }
}
