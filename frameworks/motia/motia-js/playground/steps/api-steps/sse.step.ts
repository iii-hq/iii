import { type Handlers, http, logger, type StepConfig } from 'motia'

export const config = {
  name: 'SSE Example',
  description: 'Accepts URL-encoded data and streams back random items as SSE',
  flows: ['sse-example'],
  triggers: [http('POST', '/sse')],
  enqueues: [],
} as const satisfies StepConfig

const SAFE_HEADERS = ['content-type', 'user-agent', 'accept', 'accept-language', 'content-length', 'x-request-id']

export const handler: Handlers<typeof config> = async ({ request, response }) => {
  const sanitizedHeaders = Object.fromEntries(
    SAFE_HEADERS.filter((h) => request.headers[h] != null).map((h) => [h, request.headers[h]]),
  )
  logger.info('Data received', { headers: sanitizedHeaders })
  response.status(200)
  const sseHeaders = {
    'content-type': 'text/event-stream',
    'cache-control': 'no-cache',
    connection: 'keep-alive',
  }
  response.headers(sseHeaders)
  logger.info('Headers set', { headers: sseHeaders })

  const chunks: string[] = []

  for await (const chunk of request.requestBody.stream) {
    chunks.push(Buffer.from(chunk).toString('utf-8'))
  }

  const body = chunks.join('').trim()
  const parts: Record<string, string> = {}
  for (const [key, value] of new URLSearchParams(body)) {
    parts[key] = value
  }

  const items = generateRandomItems(parts)

  for (const item of items) {
    response.stream.write(`event: item\ndata: ${JSON.stringify(item)}\n\n`)
    await sleep(300 + Math.random() * 700)
  }

  response.stream.write(`event: done\ndata: ${JSON.stringify({ total: items.length })}\n\n`)
  response.close()
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function generateRandomItems(parts: Record<string, string>) {
  const fields = Object.entries(parts).map(([name, value]) => ({ name, value }))
  const count = 5 + Math.floor(Math.random() * 6)
  const adjectives = ['swift', 'lazy', 'bold', 'calm', 'fierce', 'gentle', 'sharp', 'wild']
  const nouns = ['falcon', 'river', 'mountain', 'crystal', 'thunder', 'shadow', 'ember', 'frost']

  return Array.from({ length: count }, (_, i) => ({
    id: `item-${Date.now()}-${i}`,
    label: `${adjectives[Math.floor(Math.random() * adjectives.length)]} ${nouns[Math.floor(Math.random() * nouns.length)]}`,
    score: Math.round(Math.random() * 100),
    source: fields.length > 0 ? fields[i % fields.length] : null,
  }))
}
