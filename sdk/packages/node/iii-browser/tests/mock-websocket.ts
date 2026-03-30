type EventListenerEntry = {
  listener: EventListenerOrEventListenerObject
  options?: boolean | AddEventListenerOptions
}

export class MockWebSocket implements Pick<WebSocket, 'readyState' | 'binaryType' | 'url' | 'send' | 'close' | 'addEventListener' | 'removeEventListener'> {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readyState = MockWebSocket.CONNECTING
  binaryType: BinaryType = 'blob'
  url: string

  onopen: ((this: WebSocket, ev: Event) => void) | null = null
  onclose: ((this: WebSocket, ev: CloseEvent) => void) | null = null
  onerror: ((this: WebSocket, ev: Event) => void) | null = null
  onmessage: ((this: WebSocket, ev: MessageEvent) => void) | null = null

  closeCode: number | undefined
  closeReason: string | undefined

  readonly sentRaw: (string | ArrayBuffer)[] = []

  private listeners = new Map<string, Set<EventListenerEntry>>()

  constructor(url: string | URL) {
    this.url = typeof url === 'string' ? url : url.toString()
  }

  get sentMessages(): string[] {
    return this.sentRaw.filter((d): d is string => typeof d === 'string')
  }

  get sentParsed(): Record<string, unknown>[] {
    return this.sentMessages.map((m) => JSON.parse(m))
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    if (typeof data === 'string' || data instanceof ArrayBuffer) {
      this.sentRaw.push(data)
    }
  }

  close(code?: number, reason?: string): void {
    this.closeCode = code
    this.closeReason = reason
    this.readyState = MockWebSocket.CLOSED
  }

  addEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: boolean | AddEventListenerOptions): void {
    let set = this.listeners.get(type)
    if (!set) {
      set = new Set()
      this.listeners.set(type, set)
    }
    set.add({ listener, options })
  }

  removeEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
    const set = this.listeners.get(type)
    if (set) {
      for (const entry of set) {
        if (entry.listener === listener) {
          set.delete(entry)
          break
        }
      }
    }
  }

  // --- Test helpers ---

  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN
    const event = new Event('open')
    this.onopen?.call(this as unknown as WebSocket, event)
    this.dispatchToListeners('open', event)
  }

  simulateMessage(data: string | ArrayBuffer): void {
    const event = new MessageEvent('message', { data })
    this.onmessage?.call(this as unknown as WebSocket, event)
    this.dispatchToListeners('message', event)
  }

  simulateClose(code = 1000, reason = ''): void {
    this.readyState = MockWebSocket.CLOSED
    const event = new CloseEvent('close', { code, reason, wasClean: true })
    this.onclose?.call(this as unknown as WebSocket, event)
    this.dispatchToListeners('close', event)
  }

  simulateError(): void {
    const event = new Event('error')
    this.onerror?.call(this as unknown as WebSocket, event)
    this.dispatchToListeners('error', event)
  }

  findSent(type: string): Record<string, unknown> | undefined {
    return this.sentParsed.find((m) => m.type === type)
  }

  findAllSent(type: string): Record<string, unknown>[] {
    return this.sentParsed.filter((m) => m.type === type)
  }

  private dispatchToListeners(type: string, event: Event): void {
    const set = this.listeners.get(type)
    if (!set) return
    for (const { listener } of set) {
      if (typeof listener === 'function') {
        listener(event)
      } else {
        listener.handleEvent(event)
      }
    }
  }
}

export class MockEngine {
  sockets: MockWebSocket[] = []
  private originalWebSocket: typeof globalThis.WebSocket | undefined
  private autoOpen: boolean

  constructor(options?: { autoOpen?: boolean }) {
    this.autoOpen = options?.autoOpen ?? true
  }

  install(): void {
    this.originalWebSocket = globalThis.WebSocket

    const engine = this
    const autoOpen = this.autoOpen

    const MockWS = class extends MockWebSocket {
      constructor(url: string | URL) {
        super(url)
        engine.sockets.push(this)
        if (autoOpen) {
          queueMicrotask(() => this.simulateOpen())
        }
      }
    }

    globalThis.WebSocket = MockWS as unknown as typeof WebSocket
  }

  uninstall(): void {
    if (this.originalWebSocket) {
      globalThis.WebSocket = this.originalWebSocket
      this.originalWebSocket = undefined
    }
    this.sockets = []
  }

  get socket(): MockWebSocket {
    const s = this.sockets[this.sockets.length - 1]
    if (!s) throw new Error('No MockWebSocket created yet')
    return s
  }

  get sentParsed(): Record<string, unknown>[] {
    return this.socket.sentParsed
  }

  async waitForOpen(): Promise<void> {
    await new Promise<void>((resolve) => queueMicrotask(resolve))
    await new Promise<void>((resolve) => queueMicrotask(resolve))
  }

  respondToInvocation(invocationId: string, result: unknown): void {
    this.socket.simulateMessage(JSON.stringify({
      type: 'invocationresult',
      invocation_id: invocationId,
      function_id: '',
      result,
    }))
  }

  respondWithError(invocationId: string, error: { code: string; message: string }): void {
    this.socket.simulateMessage(JSON.stringify({
      type: 'invocationresult',
      invocation_id: invocationId,
      function_id: '',
      error,
    }))
  }

  invokeFunction(functionId: string, data: unknown, invocationId?: string): void {
    this.socket.simulateMessage(JSON.stringify({
      type: 'invokefunction',
      invocation_id: invocationId ?? crypto.randomUUID(),
      function_id: functionId,
      data,
    }))
  }

  invokeFunctionVoid(functionId: string, data: unknown): void {
    this.socket.simulateMessage(JSON.stringify({
      type: 'invokefunction',
      function_id: functionId,
      data,
    }))
  }

  sendWorkerRegistered(workerId?: string): void {
    this.socket.simulateMessage(JSON.stringify({
      type: 'workerregistered',
      worker_id: workerId ?? `worker-${crypto.randomUUID().slice(0, 8)}`,
    }))
  }

  sendRegisterTrigger(triggerType: string, id: string, functionId: string, config: unknown): void {
    this.socket.simulateMessage(JSON.stringify({
      type: 'registertrigger',
      trigger_type: triggerType,
      id,
      function_id: functionId,
      config,
    }))
  }

  findSent(type: string): Record<string, unknown> | undefined {
    return this.socket.findSent(type)
  }

  findAllSent(type: string): Record<string, unknown>[] {
    return this.socket.findAllSent(type)
  }

  autoRespondToInvocations(): void {
    const origSimulateOpen = this.socket.simulateOpen.bind(this.socket)
    const socket = this.socket
    const engine = this

    const originalSend = socket.send.bind(socket)
    socket.send = (data: string | ArrayBufferLike | Blob | ArrayBufferView) => {
      originalSend(data)
      if (typeof data === 'string') {
        try {
          const parsed = JSON.parse(data)
          if (parsed.type === 'invokefunction' && parsed.invocation_id) {
            queueMicrotask(() => {
              engine.respondToInvocation(parsed.invocation_id, { functions: [], workers: [], triggers: [], trigger_types: [] })
            })
          }
        } catch {}
      }
    }
  }
}
