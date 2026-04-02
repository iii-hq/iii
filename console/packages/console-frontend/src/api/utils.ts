import { getDevtoolsApi } from './config'

interface WrappedResponse<T> {
  status_code: number
  headers: [string, string][]
  body: T
}

const CORS_ERROR_MESSAGE =
  'Connection blocked by CORS. Check iii-engine CORS settings for this console origin.'

async function unwrapResponse<T>(res: Response): Promise<T> {
  const data = await res.json()

  if (data && typeof data === 'object' && 'status_code' in data && 'body' in data) {
    const wrapped = data as WrappedResponse<T>
    if (wrapped.status_code !== 200) {
      throw new Error(`API Error: ${JSON.stringify(wrapped.body)}`)
    }
    return wrapped.body
  }

  return data as T
}

export function isCorsLikeFetchError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false
  }

  if (error.name === 'AbortError') {
    return false
  }

  const message = error.message.toLowerCase()

  return (
    message.includes('failed to fetch') ||
    message.includes('networkerror') ||
    message.includes('cors') ||
    message.includes('cross-origin') ||
    message.includes('access-control-allow-origin')
  )
}

export function getConnectionErrorMessage(error: unknown, fallback = 'Network error'): string {
  if (isCorsLikeFetchError(error)) {
    return CORS_ERROR_MESSAGE
  }

  if (error instanceof Error && error.message) {
    return error.message
  }

  return fallback
}

export async function fetchWithFallback<T>(
  devtoolsPath: string,
  _managementPath?: string,
  options?: RequestInit,
): Promise<T> {
  try {
    const res = await fetch(`${getDevtoolsApi()}${devtoolsPath}`, options)
    if (!res.ok) {
      throw new Error(`Failed to fetch from ${devtoolsPath}: ${res.status}`)
    }
    return await unwrapResponse<T>(res)
  } catch (error) {
    throw new Error(getConnectionErrorMessage(error))
  }
}

export type { WrappedResponse }
export { CORS_ERROR_MESSAGE, unwrapResponse }
