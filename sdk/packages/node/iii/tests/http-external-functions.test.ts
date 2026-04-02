import { createServer, type IncomingHttpHeaders } from 'node:http'
import type { AddressInfo } from 'node:net'
import { describe, expect, it } from 'vitest'
import { execute, iii, sleep } from './utils'

type CapturedWebhook = {
  method: string
  url: string
  headers: IncomingHttpHeaders
  body: unknown
  rawBody: string
}

class WebhookProbe {
  private server = createServer(async (req, res) => {
    const chunks: Buffer[] = []
    for await (const chunk of req) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
    }

    const rawBody = Buffer.concat(chunks).toString('utf8')
    let body: unknown = rawBody
    if (rawBody) {
      try {
        body = JSON.parse(rawBody)
      } catch {
        body = rawBody
      }
    } else {
      body = null
    }

    const captured: CapturedWebhook = {
      method: req.method ?? 'POST',
      url: req.url ?? '/',
      headers: req.headers,
      body,
      rawBody,
    }

    const waiter = this.waiters.shift()
    if (waiter) {
      waiter(captured)
    } else {
      this.queue.push(captured)
    }

    res.writeHead(200, { 'content-type': 'application/json' })
    res.end(JSON.stringify({ ok: true }))
  })

  private queue: CapturedWebhook[] = []
  private waiters: Array<(payload: CapturedWebhook) => void> = []

  async start(): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      const onError = (error: Error) => {
        this.server.off('error', onError)
        reject(error)
      }

      this.server.once('error', onError)
      this.server.listen(0, '127.0.0.1', () => {
        this.server.off('error', onError)
        resolve()
      })
    })
  }

  async close(): Promise<void> {
    if (!this.server.listening) {
      return
    }

    await new Promise<void>((resolve, reject) => {
      this.server.close(error => {
        if (error) {
          reject(error)
          return
        }
        resolve()
      })
    })
  }

  url(path = '/webhook'): string {
    const address = this.server.address()
    if (!address || typeof address === 'string') {
      throw new Error('Webhook server is not listening')
    }

    const { port } = address as AddressInfo
    return `http://127.0.0.1:${port}${path}`
  }

  async waitForWebhook(timeoutMs = 5000): Promise<CapturedWebhook> {
    if (this.queue.length > 0) {
      const next = this.queue.shift()
      if (next) {
        return next
      }
    }

    return new Promise<CapturedWebhook>((resolve, reject) => {
      const waiter = (payload: CapturedWebhook) => {
        clearTimeout(timeout)
        const idx = this.waiters.indexOf(waiter)
        if (idx >= 0) this.waiters.splice(idx, 1)
        resolve(payload)
      }

      const timeout = setTimeout(() => {
        const idx = this.waiters.indexOf(waiter)
        if (idx >= 0) this.waiters.splice(idx, 1)
        reject(new Error(`Timeout waiting for webhook after ${timeoutMs}ms`))
      }, timeoutMs)

      this.waiters.push(waiter)
    })
  }
}

function uniqueFunctionId(prefix: string): string {
  return `${prefix}::${Date.now()}::${Math.random().toString(36).slice(2, 10)}`
}

function uniqueTopic(prefix: string): string {
  return `${prefix}.${Date.now()}.${Math.random().toString(36).slice(2, 10)}`
}

describe('HTTP external functions', () => {
  it('delivers queue events to an externally registered HTTP function', async () => {
    await execute(async () => iii.listFunctions())

    const webhookProbe = new WebhookProbe()
    await webhookProbe.start()

    const functionId = uniqueFunctionId('test::http_external::target')
    const topic = uniqueTopic('test.http_external.topic')
    const payload = { hello: 'world', count: 1 }
    let trigger: { unregister(): void } | undefined
    let httpFn: { unregister(): void } | undefined

    try {
      httpFn = iii.registerFunction(
        { id: functionId },
        {
          url: webhookProbe.url(),
          method: 'POST',
          timeout_ms: 3000,
        },
      )
      await sleep(300)

      trigger = iii.registerTrigger({
        type: 'queue',
        function_id: functionId,
        config: { topic },
      })
      await sleep(300)

      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic, data: payload } }))

      const webhook = await webhookProbe.waitForWebhook(7000)

      expect(webhook.method).toBe('POST')
      expect(webhook.url).toBe('/webhook')
      expect(webhook.body).toMatchObject(payload)
    } finally {
      try {
        trigger?.unregister()
      } finally {
        httpFn?.unregister()
        await webhookProbe.close()
      }
    }
  })

  it('registers and unregisters an HTTP function', async () => {
    await execute(async () => iii.listFunctions())

    const webhookProbe = new WebhookProbe()
    await webhookProbe.start()

    const functionId = uniqueFunctionId('test::http_external::register_unregister')
    let httpFn: { id: string; unregister(): void } | undefined

    try {
      httpFn = iii.registerFunction(
        { id: functionId },
        {
          url: webhookProbe.url(),
          method: 'POST',
          timeout_ms: 3000,
        },
      )
      await sleep(300)

      const functionsAfterRegister = await execute(async () => iii.listFunctions())
      const registered = functionsAfterRegister.find(f => f.function_id === functionId)
      expect(registered).toBeDefined()

      httpFn.unregister()
      httpFn = undefined
      await sleep(300)

      const functionsAfterUnregister = await execute(async () => iii.listFunctions())
      const unregistered = functionsAfterUnregister.find(f => f.function_id === functionId)
      expect(unregistered).toBeUndefined()
    } finally {
      httpFn?.unregister()
      await webhookProbe.close()
    }
  })

  it('delivers events with custom headers to the webhook', async () => {
    await execute(async () => iii.listFunctions())

    const webhookProbe = new WebhookProbe()
    await webhookProbe.start()

    const functionId = uniqueFunctionId('test::http_external::custom_headers')
    const topic = uniqueTopic('test.http_external.headers')
    const payload = { msg: 'with-headers' }
    let trigger: { unregister(): void } | undefined
    let httpFn: { unregister(): void } | undefined

    try {
      httpFn = iii.registerFunction(
        { id: functionId },
        {
          url: webhookProbe.url(),
          method: 'POST',
          timeout_ms: 3000,
          headers: {
            'X-Custom-Header': 'test-value',
            'X-Another': '123',
          },
        },
      )
      await sleep(300)

      trigger = iii.registerTrigger({
        type: 'queue',
        function_id: functionId,
        config: { topic },
      })
      await sleep(300)

      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic, data: payload } }))

      const webhook = await webhookProbe.waitForWebhook(7000)

      expect(webhook.method).toBe('POST')
      expect(webhook.body).toMatchObject(payload)
      expect(webhook.headers['x-custom-header']).toBe('test-value')
      expect(webhook.headers['x-another']).toBe('123')
    } finally {
      try {
        trigger?.unregister()
      } finally {
        httpFn?.unregister()
        await webhookProbe.close()
      }
    }
  })

  it('delivers events to multiple external functions on different topics', async () => {
    await execute(async () => iii.listFunctions())

    const webhookProbeA = new WebhookProbe()
    const webhookProbeB = new WebhookProbe()
    await webhookProbeA.start()
    await webhookProbeB.start()

    const functionIdA = uniqueFunctionId('test::http_external::multi_a')
    const functionIdB = uniqueFunctionId('test::http_external::multi_b')
    const topicA = uniqueTopic('test.http_external.multi_a')
    const topicB = uniqueTopic('test.http_external.multi_b')
    const payloadA = { source: 'topic-a', value: 1 }
    const payloadB = { source: 'topic-b', value: 2 }

    let triggerA: { unregister(): void } | undefined
    let triggerB: { unregister(): void } | undefined
    let httpFnA: { unregister(): void } | undefined
    let httpFnB: { unregister(): void } | undefined

    try {
      httpFnA = iii.registerFunction(
        { id: functionIdA },
        {
          url: webhookProbeA.url(),
          method: 'POST',
          timeout_ms: 3000,
        },
      )

      httpFnB = iii.registerFunction(
        { id: functionIdB },
        {
          url: webhookProbeB.url(),
          method: 'POST',
          timeout_ms: 3000,
        },
      )
      await sleep(300)

      triggerA = iii.registerTrigger({
        type: 'queue',
        function_id: functionIdA,
        config: { topic: topicA },
      })

      triggerB = iii.registerTrigger({
        type: 'queue',
        function_id: functionIdB,
        config: { topic: topicB },
      })
      await sleep(300)

      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic: topicA, data: payloadA } }))
      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic: topicB, data: payloadB } }))

      const webhookA = await webhookProbeA.waitForWebhook(7000)
      const webhookB = await webhookProbeB.waitForWebhook(7000)

      expect(webhookA.method).toBe('POST')
      expect(webhookA.body).toMatchObject(payloadA)

      expect(webhookB.method).toBe('POST')
      expect(webhookB.body).toMatchObject(payloadB)
    } finally {
      try {
        triggerA?.unregister()
        triggerB?.unregister()
      } finally {
        httpFnA?.unregister()
        httpFnB?.unregister()
        await webhookProbeA.close()
        await webhookProbeB.close()
      }
    }
  })

  it('stops delivering events after unregister', async () => {
    await execute(async () => iii.listFunctions())

    const webhookProbe = new WebhookProbe()
    await webhookProbe.start()

    const functionId = uniqueFunctionId('test::http_external::stop_after_unregister')
    const topic = uniqueTopic('test.http_external.stop')
    const payloadBefore = { phase: 'before-unregister' }
    const payloadAfter = { phase: 'after-unregister' }
    let trigger: { unregister(): void } | undefined
    let httpFn: { unregister(): void } | undefined

    try {
      httpFn = iii.registerFunction(
        { id: functionId },
        {
          url: webhookProbe.url(),
          method: 'POST',
          timeout_ms: 3000,
        },
      )
      await sleep(300)

      trigger = iii.registerTrigger({
        type: 'queue',
        function_id: functionId,
        config: { topic },
      })
      await sleep(300)

      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic, data: payloadBefore } }))

      const webhookBefore = await webhookProbe.waitForWebhook(7000)
      expect(webhookBefore.body).toMatchObject(payloadBefore)

      trigger.unregister()
      trigger = undefined
      httpFn.unregister()
      httpFn = undefined
      await sleep(500)

      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic, data: payloadAfter } }))

      let receivedAfterUnregister = false
      try {
        await webhookProbe.waitForWebhook(2000)
        receivedAfterUnregister = true
      } catch {
        receivedAfterUnregister = false
      }

      expect(receivedAfterUnregister).toBe(false)
    } finally {
      try {
        trigger?.unregister()
      } finally {
        httpFn?.unregister()
        await webhookProbe.close()
      }
    }
  })

  it('delivers events using PUT method', async () => {
    await execute(async () => iii.listFunctions())

    const webhookProbe = new WebhookProbe()
    await webhookProbe.start()

    const functionId = uniqueFunctionId('test::http_external::put_method')
    const topic = uniqueTopic('test.http_external.put')
    const payload = { method_test: 'put', value: 42 }
    let trigger: { unregister(): void } | undefined
    let httpFn: { unregister(): void } | undefined

    try {
      httpFn = iii.registerFunction(
        { id: functionId },
        {
          url: webhookProbe.url(),
          method: 'PUT',
          timeout_ms: 3000,
        },
      )
      await sleep(300)

      trigger = iii.registerTrigger({
        type: 'queue',
        function_id: functionId,
        config: { topic },
      })
      await sleep(300)

      await execute(async () => iii.trigger({ function_id: 'enqueue', payload: { topic, data: payload } }))

      const webhook = await webhookProbe.waitForWebhook(7000)

      expect(webhook.method).toBe('PUT')
      expect(webhook.url).toBe('/webhook')
      expect(webhook.body).toMatchObject(payload)
    } finally {
      try {
        trigger?.unregister()
      } finally {
        httpFn?.unregister()
        await webhookProbe.close()
      }
    }
  })
})
