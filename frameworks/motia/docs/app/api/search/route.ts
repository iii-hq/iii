import { createFromSource } from 'fumadocs-core/search/server'
import { baseSource } from '@/lib/source'

const { GET: baseGET } = createFromSource(baseSource)
const isProduction = process.env.NODE_ENV === 'production'

function isLegacyExamplesUrl(value: unknown) {
  return typeof value === 'string' && /^\/docs\/examples(?:\/|$)/.test(value)
}

function removeLegacyExamples(data: unknown): unknown {
  if (Array.isArray(data)) {
    return data
      .filter((item) => {
        if (item && typeof item === 'object') {
          const candidate = item as Record<string, unknown>
          return !isLegacyExamplesUrl(candidate.url) && !isLegacyExamplesUrl(candidate.path)
        }
        return true
      })
      .map(removeLegacyExamples)
  }

  if (data && typeof data === 'object') {
    const record = data as Record<string, unknown>
    return Object.fromEntries(Object.entries(record).map(([key, value]) => [key, removeLegacyExamples(value)]))
  }

  return data
}

export async function GET(request: Request) {
  const response = await baseGET(request)
  const responseClone = response.clone()
  if (!isProduction) return response

  try {
    const payload = await response.json()
    const filteredPayload = removeLegacyExamples(payload)
    const headers = new Headers(response.headers)
    headers.set('Content-Type', 'application/json')
    return new Response(JSON.stringify(filteredPayload), {
      status: response.status,
      headers,
    })
  } catch {
    return responseClone
  }
}
