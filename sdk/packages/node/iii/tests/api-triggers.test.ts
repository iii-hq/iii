import * as fs from 'node:fs'
import * as path from 'node:path'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { http, type ApiResponse, type HttpRequest } from '../src'
import type { HttpResponse } from '../src/types'
import { engineHttpUrl, execute, httpRequest, iii, sleep } from './utils'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const pdfPath = path.join(__dirname, '..', '..', '..', '..', 'test-assets', 'handbook.pdf')

describe('API Triggers', () => {
  it('should register GET endpoint', async () => {
    const fn = iii.registerFunction(
      { id: 'test.api.get' },
      async (_req: HttpRequest): Promise<ApiResponse> => ({
        status_code: 200,
        body: { message: 'Hello from GET' },
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/hello',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await execute(async () => httpRequest('GET', '/test/hello'))

    expect(response.status).toBe(200)
    expect(response.data).toEqual({ message: 'Hello from GET' })

    fn.unregister()
    trigger.unregister()
  })

  it('should register POST endpoint with body', async () => {
    const fn = iii.registerFunction(
      { id: 'test.api.post' },
      async (req: HttpRequest): Promise<ApiResponse> => {
        const body = (req.body as Record<string, unknown>) ?? {}
        return {
          status_code: 201,
          body: { received: body, created: true },
        }
      },
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/items',
        http_method: 'POST',
      },
    })

    await sleep(300)

    const response = await httpRequest('POST', '/test/items', { name: 'test item', value: 123 })

    expect(response.status).toBe(201)
    expect(response.data.created).toBe(true)
    expect(response.data.received).toHaveProperty('name', 'test item')

    fn.unregister()
    trigger.unregister()
  })

  it('should handle path parameters', async () => {
    const fn = iii.registerFunction(
      { id: 'test.api.getById' },
      async (req: HttpRequest): Promise<ApiResponse> => ({
        status_code: 200,
        body: { id: req.path_params?.id },
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/items/:id',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await execute(async () => httpRequest('GET', '/test/items/abc123'))

    expect(response.status).toBe(200)
    expect(response.data).toEqual({ id: 'abc123' })

    fn.unregister()
    trigger.unregister()
  })

  it('should handle query parameters', async () => {
    const fn = iii.registerFunction(
      { id: 'test.api.search' },
      async (req: HttpRequest): Promise<ApiResponse> => {
        const q = req.query_params?.q
        const limit = req.query_params?.limit
        const qVal = Array.isArray(q) ? q[0] : q
        const limitVal = Array.isArray(limit) ? limit[0] : limit
        return {
          status_code: 200,
          body: { query: qVal, limit: limitVal },
        }
      },
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/search',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await execute(async () => httpRequest('GET', '/test/search?q=hello&limit=10'))

    expect(response.status).toBe(200)
    expect(response.data.query).toBe('hello')
    expect(response.data.limit).toBe('10')

    fn.unregister()
    trigger.unregister()
  })

  it('should return custom status code', async () => {
    const fn = iii.registerFunction(
      { id: 'test.api.notfound' },
      async (_req: HttpRequest): Promise<ApiResponse<404>> => ({
        status_code: 404,
        body: { error: 'Not found' },
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/missing',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await httpRequest('GET', '/test/missing')

    expect(response.status).toBe(404)
    expect(response.data).toEqual({ error: 'Not found' })

    fn.unregister()
    trigger.unregister()
  })

  it('should download a PDF file via streaming response', async () => {
    const originalPdf = fs.readFileSync(pdfPath)
    const fn = iii.registerFunction(
      { id: 'test.api.download.pdf' },
      http(async (_req: HttpRequest, response: HttpResponse) => {
        const fileStream = fs.createReadStream(pdfPath)

        response.status(200)
        response.headers({ 'content-type': 'application/pdf' })

        await pipeline(fileStream, response.stream)
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/download/pdf',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await fetch(`${engineHttpUrl}/test/download/pdf`)
    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toBe('application/pdf')

    const downloadedBuffer = Buffer.from(await response.arrayBuffer())
    expect(downloadedBuffer.length).toBe(originalPdf.length)
    expect(downloadedBuffer.equals(originalPdf)).toBe(true)

    fn.unregister()
    trigger.unregister()
  })

  it.skip('should upload a PDF file via streaming request', async () => {
    const originalPdf = fs.readFileSync(pdfPath)

    let receivedBuffer: Buffer = null as never

    const fn = iii.registerFunction(
      { id: 'test.api.upload.pdf' },
      http(async (req: HttpRequest, response: HttpResponse) => {
        const chunks: Buffer[] = []

        response.status(200)
        response.headers({ 'content-type': 'application/json' })

        for await (const chunk of req.request_body.stream) {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
        }

        receivedBuffer = Buffer.concat(chunks)

        response.stream.end(Buffer.from(JSON.stringify({ received_size: receivedBuffer.length })))
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/upload/pdf',
        http_method: 'POST',
      },
    })

    await sleep(300)

    const response = await fetch(`${engineHttpUrl}/test/upload/pdf`, {
      method: 'POST',
      headers: { 'content-type': 'application/octet-stream' },
      body: originalPdf,
    })

    expect(response.status).toBe(200)

    const data = (await response.json()) as Record<string, unknown>

    expect(data.received_size).toBe(originalPdf.length)
    expect(receivedBuffer).not.toBeNull()
    expect(receivedBuffer.equals(originalPdf)).toBe(true)

    fn.unregister()
    trigger.unregister()
  })

  it('should stream SSE events', async () => {
    const events = [
      { id: '1', type: 'message', data: 'Hello, world!' },
      { id: '2', type: 'update', data: JSON.stringify({ count: 42 }) },
      { id: '3', type: 'message', data: 'line one\nline two' },
      { id: '4', type: 'done', data: 'goodbye' },
    ]

    const fn = iii.registerFunction(
      { id: 'test.api.sse' },
      http(async (_req: HttpRequest, response: HttpResponse) => {
        response.status(200)
        response.headers({
          'content-type': 'text/event-stream',
          'cache-control': 'no-cache',
          connection: 'keep-alive',
        })

        for (const event of events) {
          let frame = ''
          frame += `id: ${event.id}\n`
          frame += `event: ${event.type}\n`
          for (const line of event.data.split('\n')) {
            frame += `data: ${line}\n`
          }
          frame += '\n'

          response.stream.write(Buffer.from(frame))
          await sleep(50)
        }

        response.stream.end()
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/sse',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await fetch(`${engineHttpUrl}/test/sse`)

    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toBe('text/event-stream')

    const body = await response.text()
    const receivedEvents: { id: string; type: string; data: string }[] = []

    for (const block of body.split('\n\n').filter(Boolean)) {
      const lines = block.split('\n')
      const ev: Record<string, string> = {}
      const dataLines: string[] = []

      for (const line of lines) {
        if (line.startsWith('id: ')) ev.id = line.slice(4)
        else if (line.startsWith('event: ')) ev.type = line.slice(7)
        else if (line.startsWith('data: ')) dataLines.push(line.slice(6))
      }

      ev.data = dataLines.join('\n')
      receivedEvents.push(ev as { id: string; type: string; data: string })
    }

    expect(receivedEvents).toHaveLength(events.length)

    for (let i = 0; i < events.length; i++) {
      expect(receivedEvents[i].id).toBe(events[i].id)
      expect(receivedEvents[i].type).toBe(events[i].type)
      expect(receivedEvents[i].data).toBe(events[i].data)
    }

    fn.unregister()
    trigger.unregister()
  })

  it('should handle application/x-www-form-urlencoded request', async () => {
    const fn = iii.registerFunction(
      { id: 'test.api.form.urlencoded' },
      http(async (req: HttpRequest, response: HttpResponse) => {
        const chunks: Buffer[] = []

        for await (const chunk of req.request_body.stream) {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
        }

        const body = Buffer.concat(chunks).toString('utf-8')
        const params = new URLSearchParams(body)

        response.status(200)
        response.headers({ 'content-type': 'application/json' })
        response.stream.end(
          Buffer.from(
            JSON.stringify({
              name: params.get('name'),
              email: params.get('email'),
              age: params.get('age'),
            }),
          ),
        )
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/form/urlencoded',
        http_method: 'POST',
      },
    })

    await sleep(300)

    const formBody = new URLSearchParams({
      name: 'John Doe',
      email: 'john@example.com',
      age: '30',
    })

    const response = await fetch(`${engineHttpUrl}/test/form/urlencoded`, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: formBody.toString(),
    })

    expect(response.status).toBe(200)
    const data = (await response.json()) as Record<string, unknown>

    expect(data.name).toBe('John Doe')
    expect(data.email).toBe('john@example.com')
    expect(data.age).toBe('30')

    fn.unregister()
    trigger.unregister()
  })

  it('should handle multipart/form-data with file upload', async () => {
    const originalPdf = fs.readFileSync(pdfPath)

    const fn = iii.registerFunction(
      { id: 'test.api.form.multipart' },
      http(async (req: HttpRequest, response: HttpResponse) => {
        const chunks: Buffer[] = []

        for await (const chunk of req.request_body.stream) {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
        }

        const rawBody = Buffer.concat(chunks)
        const contentType = (req.headers?.['content-type'] as string) ?? ''
        const boundaryMatch = contentType.match(/boundary=([^\s;]+)/)
        const boundary = boundaryMatch?.[1] ?? ''

        const bodyText = rawBody.toString()
        const hasTitle = bodyText.includes('Test Document')
        const hasDescription = bodyText.includes('A test upload')
        const hasFilename = bodyText.includes('filename="handbook.pdf"')

        response.status(200)
        response.headers({ 'content-type': 'application/json' })
        response.stream.end(
          Buffer.from(
            JSON.stringify({
              has_boundary: boundary.length > 0,
              has_title: hasTitle,
              has_description: hasDescription,
              has_filename: hasFilename,
              body_size: rawBody.length,
            }),
          ),
        )
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/form/multipart',
        http_method: 'POST',
      },
    })

    await sleep(300)

    const formData = new FormData()
    formData.append('title', 'Test Document')
    formData.append('description', 'A test upload')
    formData.append('file', new Blob([originalPdf]), 'handbook.pdf')

    const response = await fetch(`${engineHttpUrl}/test/form/multipart`, {
      method: 'POST',
      body: formData,
    })

    expect(response.status).toBe(200)
    const data = (await response.json()) as Record<string, unknown>

    expect(data.has_boundary).toBe(true)
    expect(data.has_title).toBe(true)
    expect(data.has_description).toBe(true)
    expect(data.has_filename).toBe(true)
    expect(data.body_size).toBeGreaterThan(originalPdf.length)

    fn.unregister()
    trigger.unregister()
  })
})
