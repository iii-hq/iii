declare const Response: any
declare const crypto: any
declare const btoa: (data: string) => string

export type FunctionHandler = (data: any, ctx: InvocationContext) => Promise<any>
export type TriggerHandler = (payload: any, ctx: TriggerContext) => Promise<any>

export type InvocationContext = {
  invocationId?: string
  functionPath?: string
  timestamp?: number
  traceId?: string
  headers: Record<string, string>
}

export type TriggerContext = {
  triggerId?: string
  triggerType?: string
  functionPath?: string
  timestamp?: number
  headers: Record<string, string>
}

export type ServerlessConfig = {
  hmacSecret?: string
  maxSignatureAge?: number
}

export class ServerlessWorker {
  private config: ServerlessConfig
  private functions: Map<string, FunctionHandler> = new Map()
  private triggers: Map<string, TriggerHandler> = new Map()

  constructor(config: ServerlessConfig = {}) {
    this.config = {
      maxSignatureAge: 300,
      ...config,
    }
  }

  function(path: string, handler: FunctionHandler): this {
    this.functions.set(path, handler)
    return this
  }

  trigger(id: string, handler: TriggerHandler): this {
    this.triggers.set(id, handler)
    return this
  }

  handler() {
    return {
      fetch: async (request: any, env: Record<string, string> = {}) => {
        if (this.config.hmacSecret) {
          const secret = resolveSecret(this.config.hmacSecret, env)
          const valid = await verifySignature(request, {
            secret,
            maxAge: this.config.maxSignatureAge ?? 300,
          })
          if (!valid) {
            return jsonResponse({ error: { code: 'unauthorized', message: 'Invalid signature' } }, 401)
          }
        }

        const triggerType = request.headers.get('X-III-Trigger-Type')
        if (triggerType) {
          return this.handleTrigger(request)
        }
        return this.handleInvocation(request)
      },
    }
  }

  private async handleInvocation(request: any): Promise<any> {
    const functionPath = request.headers.get('X-III-Function-Path') ?? ''
    const invocationId = request.headers.get('X-III-Invocation-ID') ?? undefined
    const traceId = request.headers.get('X-III-Trace-ID') ?? undefined
    const timestamp = parseInt(request.headers.get('X-III-Timestamp') ?? '', 10)

    const handler = this.functions.get(functionPath)
    if (!handler) {
      return jsonResponse({ error: { code: 'not_found', message: 'Function not found' } }, 404)
    }

    const body = await safeJson(request)
    const ctx: InvocationContext = {
      invocationId,
      functionPath,
      timestamp: Number.isFinite(timestamp) ? timestamp : undefined,
      traceId,
      headers: headersToRecord(request.headers),
    }

    try {
      const result = await handler(body, ctx)
      return jsonResponse(result ?? null, 200)
    } catch (err: any) {
      return jsonResponse(
        { error: { code: 'internal_error', message: err?.message ?? 'Internal error' } },
        500,
      )
    }
  }

  private async handleTrigger(request: any): Promise<any> {
    const triggerId = request.headers.get('X-III-Trigger-ID') ?? ''
    const triggerType = request.headers.get('X-III-Trigger-Type') ?? undefined
    const functionPath = request.headers.get('X-III-Function-Path') ?? undefined
    const timestamp = parseInt(request.headers.get('X-III-Timestamp') ?? '', 10)

    const handler = this.triggers.get(triggerId)
    if (!handler) {
      return jsonResponse({ error: { code: 'not_found', message: 'Trigger not found' } }, 404)
    }

    const payload = await safeJson(request)
    const ctx: TriggerContext = {
      triggerId,
      triggerType,
      functionPath,
      timestamp: Number.isFinite(timestamp) ? timestamp : undefined,
      headers: headersToRecord(request.headers),
    }

    try {
      const result = await handler(payload, ctx)
      return jsonResponse(result ?? null, 200)
    } catch (err: any) {
      return jsonResponse(
        { error: { code: 'internal_error', message: err?.message ?? 'Internal error' } },
        500,
      )
    }
  }
}

export class EngineClient {
  private engineUrl: string
  private token: string

  constructor(config: { engineUrl: string; token: string }) {
    this.engineUrl = config.engineUrl
    this.token = config.token
  }

  async invoke(functionPath: string, data: any): Promise<any> {
    const response = await fetch(`${this.engineUrl}/api/invoke`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${this.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ function_path: functionPath, data }),
    })
    if (!response.ok) {
      throw new Error(`Invocation failed: ${response.status}`)
    }
    return response.json()
  }

  async state(action: 'get' | 'set' | 'delete', key: string, value?: any): Promise<any> {
    const response = await fetch(`${this.engineUrl}/api/state/${action}`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${this.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ key, value }),
    })
    if (!response.ok) {
      throw new Error(`State request failed: ${response.status}`)
    }
    return response.json()
  }

  async emit(topic: string, data: any): Promise<void> {
    const response = await fetch(`${this.engineUrl}/api/events/emit`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${this.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ topic, data }),
    })
    if (!response.ok) {
      throw new Error(`Emit failed: ${response.status}`)
    }
  }
}

export async function verifySignature(
  request: any,
  options: { secret: string; maxAge: number },
): Promise<boolean> {
  const signature = request.headers.get('X-III-Signature') ?? ''
  const timestampHeader = request.headers.get('X-III-Timestamp') ?? ''
  const timestamp = parseInt(timestampHeader, 10)
  if (!signature || !Number.isFinite(timestamp)) {
    return false
  }

  const now = Math.floor(Date.now() / 1000)
  if (now - timestamp > options.maxAge) {
    return false
  }

  const body = await request.clone().arrayBuffer()
  const expected = await signRequest(new Uint8Array(body), options.secret, timestamp)
  return timingSafeEqual(signature, expected)
}

export async function parseInvocation(request: any): Promise<{ functionPath: string; data: any }> {
  const functionPath = request.headers.get('X-III-Function-Path') ?? ''
  const data = await safeJson(request)
  return { functionPath, data }
}

export async function parseTrigger(request: any): Promise<{ triggerId: string; payload: any }> {
  const triggerId = request.headers.get('X-III-Trigger-ID') ?? ''
  const payload = await safeJson(request)
  return { triggerId, payload }
}

async function signRequest(body: Uint8Array, secret: string, timestamp: number): Promise<string> {
  const bodyB64 = base64Encode(body)
  const payload = `${timestamp}:${bodyB64}`
  const key = await crypto.subtle.importKey(
    'raw',
    new TextEncoder().encode(secret),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  )
  const signature = await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(payload))
  return `sha256=${hexEncode(new Uint8Array(signature))}`
}

function resolveSecret(secretOrEnv: string, env: Record<string, string>): string {
  return env[secretOrEnv] ?? secretOrEnv
}

async function safeJson(request: any): Promise<any> {
  try {
    return await request.json()
  } catch {
    return null
  }
}

function headersToRecord(headers: any): Record<string, string> {
  const out: Record<string, string> = {}
  headers.forEach((value, key) => {
    out[key] = value
  })
  return out
}

function jsonResponse(body: any, status = 200): any {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function base64Encode(data: Uint8Array): string {
  if (typeof Buffer !== 'undefined') {
    return Buffer.from(data).toString('base64')
  }
  let binary = ''
  data.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })
  return btoa(binary)
}

function hexEncode(data: Uint8Array): string {
  return Array.from(data)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}

function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false
  let result = 0
  for (let i = 0; i < a.length; i += 1) {
    result |= a.charCodeAt(i) ^ b.charCodeAt(i)
  }
  return result === 0
}
