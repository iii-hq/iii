import { context, trace } from '@opentelemetry/api'
import { createRequire } from 'node:module'
import * as os from 'node:os'
import { type Data, WebSocket } from 'ws'
import { ChannelReader, ChannelWriter } from './channels'
import {
  DEFAULT_BRIDGE_RECONNECTION_CONFIG,
  DEFAULT_INVOCATION_TIMEOUT_MS,
  EngineFunctions,
  EngineTriggers,
  type IIIConnectionState,
  type IIIReconnectionConfig,
} from './iii-constants'
import {
  type FunctionInfo,
  type HttpInvocationConfig,
  type IIIMessage,
  type InvocationResultMessage,
  type InvokeFunctionMessage,
  MessageType,
  type RegisterFunctionMessage,
  type RegisterServiceMessage,
  type RegisterTriggerMessage,
  type RegisterTriggerTypeMessage,
  type StreamChannelRef,
  type TriggerAction as TriggerActionType,
  type TriggerInfo,
  type TriggerRegistrationResultMessage,
  type TriggerRequest,
  type TriggerTypeInfo,
  type WorkerInfo,
  type WorkerRegisteredMessage,
} from './iii-types'
import { registerWorkerGauges, stopWorkerGauges } from './otel-worker-gauges'
import type { IStream } from './stream'
import {
  extractContext,
  getLogger,
  getMeter,
  getTracer,
  initOtel,
  injectBaggage,
  injectTraceparent,
  type OtelConfig,
  SeverityNumber,
  shutdownOtel,
  SpanKind,
  withSpan,
} from './telemetry-system'
import type { TriggerHandler } from './triggers'
import type {
  FunctionRef,
  FunctionsAvailableCallback,
  Invocation,
  ISdk,
  RemoteFunctionData,
  RemoteFunctionHandler,
  RemoteTriggerTypeData,
  Trigger,
  TriggerTypeRef,
} from './types'
import { isChannelRef } from './utils'

const require = createRequire(import.meta.url)
const { version: SDK_VERSION } = require('../package.json')

function getOsInfo(): string {
  return `${os.platform()} ${os.release()} (${os.arch()})`
}

function getDefaultWorkerName(): string {
  return `${os.hostname()}:${process.pid}`
}

/** @internal */
export type TelemetryOptions = {
  language?: string
  project_name?: string
  framework?: string
  amplitude_api_key?: string
}

/**
 * Configuration options passed to {@link registerWorker}.
 *
 * @example
 * ```typescript
 * const iii = registerWorker('ws://localhost:49134', {
 *   workerName: 'my-worker',
 *   invocationTimeoutMs: 10000,
 *   reconnectionConfig: { maxRetries: 5 },
 * })
 * ```
 */
export type InitOptions = {
  /** Display name for this worker. Defaults to `hostname:pid`. */
  workerName?: string
  /** Enable worker metrics via OpenTelemetry. Defaults to `true`. */
  enableMetricsReporting?: boolean
  /** Default timeout for `trigger()` in milliseconds. Defaults to `30000`. */
  invocationTimeoutMs?: number
  /**
   * WebSocket reconnection behavior.
   *
   * @see {@link IIIReconnectionConfig} for available fields and defaults.
   */
  reconnectionConfig?: Partial<IIIReconnectionConfig>
  /**
   * OpenTelemetry configuration. OTel is initialized automatically by default.
   * Set `{ enabled: false }` or env `OTEL_ENABLED=false/0/no/off` to disable.
   * The `engineWsUrl` is set automatically from the III address.
   */
  otel?: Omit<OtelConfig, 'engineWsUrl'>
  /** @internal */
  telemetry?: TelemetryOptions
}

class Sdk implements ISdk {
  private ws?: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private services = new Map<string, Omit<RegisterServiceMessage, 'functions'>>()
  private invocations = new Map<string, Invocation & { timeout?: NodeJS.Timeout }>()
  private triggers = new Map<string, RegisterTriggerMessage>()
  private triggerTypes = new Map<string, RemoteTriggerTypeData>()
  private functionsAvailableCallbacks = new Set<FunctionsAvailableCallback>()
  private functionsAvailableTrigger?: Trigger
  private functionsAvailableFunctionPath?: string
  private messagesToSend: Record<string, unknown>[] = []
  private workerName: string
  private workerId?: string
  private reconnectTimeout?: NodeJS.Timeout
  private metricsReportingEnabled: boolean
  private invocationTimeoutMs: number
  private reconnectionConfig: IIIReconnectionConfig
  private reconnectAttempt = 0
  private connectionState: IIIConnectionState = 'disconnected'
  private isShuttingDown = false

  constructor(
    private readonly address: string,
    private readonly options?: InitOptions,
  ) {
    this.workerName = options?.workerName ?? getDefaultWorkerName()
    this.metricsReportingEnabled = options?.enableMetricsReporting ?? true
    this.invocationTimeoutMs = options?.invocationTimeoutMs ?? DEFAULT_INVOCATION_TIMEOUT_MS
    this.reconnectionConfig = {
      ...DEFAULT_BRIDGE_RECONNECTION_CONFIG,
      ...options?.reconnectionConfig,
    }

    // Initialize OpenTelemetry (enabled by default, opt-out via config or env)
    initOtel({ ...options?.otel, engineWsUrl: this.address })

    this.connect()
  }

  /**
   * Registers a custom trigger type with the engine. A trigger type defines
   * how external events (HTTP, cron, queue, etc.) map to function invocations.
   *
   * @param triggerType - Trigger type registration input.
   * @param triggerType.id - Unique trigger type identifier.
   * @param triggerType.description - Human-readable description.
   * @param handler - Handler with `registerTrigger` / `unregisterTrigger` callbacks.
   *
   * @example
   * ```typescript
   * iii.registerTriggerType(
   *   { id: 'my-trigger', description: 'Custom trigger' },
   *   {
   *     async registerTrigger({ id, function_id, config }) { },
   *     async unregisterTrigger({ id, function_id, config }) { },
   *   },
   * )
   * ```
   */
  registerTriggerType = <TConfig>(
    triggerType: Omit<RegisterTriggerTypeMessage, 'message_type'>,
    handler: TriggerHandler<TConfig>,
  ): TriggerTypeRef<TConfig> => {
    this.sendMessage(MessageType.RegisterTriggerType, triggerType, true)
    this.triggerTypes.set(triggerType.id, {
      message: { ...triggerType, message_type: MessageType.RegisterTriggerType },
      handler,
    })

    return {
      id: triggerType.id,
      registerTrigger: (functionId: string, config: TConfig) => {
        return this.registerTrigger({
          type: triggerType.id,
          function_id: functionId,
          config,
        })
      },
      registerFunction: (func, handler, config) => {
        const ref = this.registerFunction(func, handler)
        this.registerTrigger({
          type: triggerType.id,
          function_id: func.id,
          config,
        })
        return ref
      },
      unregister: () => {
        this.unregisterTriggerType(triggerType)
      },
    }
  }

  /**
   * Unregisters a previously registered trigger type.
   *
   * @param triggerType - The trigger type to unregister (must match the `id` used during registration).
   */
  unregisterTriggerType = (triggerType: Omit<RegisterTriggerTypeMessage, 'message_type'>): void => {
    this.sendMessage(MessageType.UnregisterTriggerType, triggerType, true)
    this.triggerTypes.delete(triggerType.id)
  }

  /**
   * Binds a trigger configuration to a registered function. When the trigger
   * fires, the engine invokes the target function.
   *
   * @param trigger - Trigger registration input.
   * @param trigger.type - Trigger type (e.g. `http`, `queue`, `cron`).
   * @param trigger.function_id - ID of the function to invoke.
   * @param trigger.config - Trigger-specific configuration.
   * @returns A {@link Trigger} handle with an `unregister()` method.
   *
   * @example
   * ```typescript
   * const trigger = iii.registerTrigger({
   *   type: 'http',
   *   function_id: 'greet',
   *   config: { api_path: '/greet', http_method: 'GET' },
   * })
   *
   * // Later...
   * trigger.unregister()
   * ```
   */
  registerTrigger = (trigger: Omit<RegisterTriggerMessage, 'message_type' | 'id'>): Trigger => {
    const id = crypto.randomUUID()
    const fullTrigger: RegisterTriggerMessage = {
      ...trigger,
      id,
      message_type: MessageType.RegisterTrigger,
    }
    this.sendMessage(MessageType.RegisterTrigger, fullTrigger, true)
    this.triggers.set(id, fullTrigger)

    return {
      unregister: () => {
        this.sendMessage(MessageType.UnregisterTrigger, {
          id,
          message_type: MessageType.UnregisterTrigger,
          type: fullTrigger.type,
        })
        this.triggers.delete(id)
      },
    }
  }

  /**
   * Registers a function with the engine. The `id` is the unique identifier
   * used by triggers and invocations.
   *
   * Pass a handler for local execution, or an {@link HttpInvocationConfig}
   * for HTTP-invoked functions (Lambda, Cloudflare Workers, etc.).
   *
   * @param message - Function registration input.
   * @param message.id - Unique function identifier.
   * @param message.description - Human-readable description.
   * @param handlerOrInvocation - Async handler or HTTP invocation config.
   * @returns A {@link FunctionRef} with `id` and `unregister()`.
   *
   * @example
   * ```typescript
   * const fn = iii.registerFunction(
   *   { id: 'greet', description: 'Greets a user' },
   *   async (input: { name: string }) => {
   *     return { message: `Hello, ${input.name}!` }
   *   },
   * )
   * ```
   */
  registerFunction = (
    message: Omit<RegisterFunctionMessage, 'message_type'>,
    handlerOrInvocation: RemoteFunctionHandler | HttpInvocationConfig,
  ): FunctionRef => {
    if (!message.id || message.id.trim() === '') {
      throw new Error('id is required')
    }
    if (this.functions.has(message.id)) {
      throw new Error(`function id already registered: ${message.id}`)
    }

    const isHandler = typeof handlerOrInvocation === 'function'

    const fullMessage: RegisterFunctionMessage = isHandler
      ? { ...message, message_type: MessageType.RegisterFunction }
      : {
          ...message,
          message_type: MessageType.RegisterFunction,
          invocation: {
            url: handlerOrInvocation.url,
            method: handlerOrInvocation.method ?? 'POST',
            timeout_ms: handlerOrInvocation.timeout_ms,
            headers: handlerOrInvocation.headers,
            auth: handlerOrInvocation.auth,
          },
        }

    this.sendMessage(MessageType.RegisterFunction, fullMessage, true)

    if (isHandler) {
      const handler = handlerOrInvocation as RemoteFunctionHandler
      this.functions.set(message.id, {
        message: fullMessage,
        handler: async (input, traceparent?: string, baggage?: string) => {
          if (getTracer()) {
            const parentContext = extractContext(traceparent, baggage)

            return context.with(parentContext, () =>
              withSpan(`call ${message.id}`, { kind: SpanKind.SERVER }, async () => await handler(input)),
            )
          }

          const traceId = crypto.randomUUID().replace(/-/g, '')
          const spanId = crypto.randomUUID().replace(/-/g, '').slice(0, 16)
          const syntheticSpan = trace.wrapSpanContext({ traceId, spanId, traceFlags: 1 })

          return context.with(trace.setSpan(context.active(), syntheticSpan), async () => await handler(input))
        },
      })
    } else {
      this.functions.set(message.id, { message: fullMessage })
    }

    return {
      id: message.id,
      unregister: () => {
        this.sendMessage(MessageType.UnregisterFunction, { id: message.id }, true)
        this.functions.delete(message.id)
      },
    }
  }

  registerService = (message: Omit<RegisterServiceMessage, 'message_type'>): void => {
    const msg = { ...message, name: message.name ?? message.id }
    this.sendMessage(MessageType.RegisterService, msg, true)
    this.services.set(message.id, { ...msg, message_type: MessageType.RegisterService })
  }

  /**
   * Creates a streaming channel pair for worker-to-worker data transfer.
   * Returns a {@link Channel} with a local writer/reader and serializable refs
   * that can be passed as fields in invocation data to other functions.
   *
   * @param bufferSize - Optional buffer size for the channel (default: 64).
   * @returns A {@link Channel} with `writer`, `reader`, and their serializable refs.
   *
   * @example
   * ```typescript
   * const channel = await iii.createChannel()
   * channel.writer.stream.write(Buffer.from('hello'))
   * channel.writer.close()
   * ```
   */
  createChannel = async (bufferSize?: number): Promise<import('./types').Channel> => {
    const result = await this.trigger<{ buffer_size?: number }, { writer: StreamChannelRef; reader: StreamChannelRef }>(
      { function_id: 'engine::channels::create', payload: { buffer_size: bufferSize } },
    )

    return {
      writer: new ChannelWriter(this.address, result.writer),
      reader: new ChannelReader(this.address, result.reader),
      writerRef: result.writer,
      readerRef: result.reader,
    }
  }

  /**
   * Invokes a remote function. The routing behavior and return type depend
   * on the `action` field of the request.
   *
   * | `action`                      | Behavior                                           | Return type              |
   * |-------------------------------|----------------------------------------------------|-----------------------   |
   * | _(none)_                      | Synchronous -- waits for the function to return     | `Promise<TOutput>`       |
   * | `TriggerAction.Enqueue(...)` | Async via named queue -- engine acknowledges enqueue | `Promise<EnqueueResult>` |
   * | `TriggerAction.Void()`       | Fire-and-forget -- no response                      | `Promise<undefined>`     |
   *
   * @param request - The trigger request.
   * @param request.function_id - ID of the function to invoke.
   * @param request.payload - Payload to pass to the function.
   * @param request.action - Routing action. Omit for synchronous request/response.
   * @param request.timeoutMs - Override the default invocation timeout.
   * @returns The result of the function invocation.
   *
   * @example
   * ```typescript
   * import { TriggerAction } from 'iii-sdk'
   *
   * // Synchronous
   * const result = await iii.trigger({ function_id: 'get-order', payload: { id: '123' } })
   *
   * // Enqueue
   * const { messageReceiptId } = await iii.trigger({
   *   function_id: 'payments::charge',
   *   payload: { orderId: '123', amount: 49.99 },
   *   action: TriggerAction.Enqueue({ queue: 'payment' }),
   * })
   *
   * // Fire-and-forget
   * iii.trigger({
   *   function_id: 'notifications::send',
   *   payload: { userId: '123' },
   *   action: TriggerAction.Void(),
   * })
   * ```
   */
  trigger = async <TInput, TOutput>(request: TriggerRequest<TInput>): Promise<TOutput> => {
    const { function_id, payload, action, timeoutMs } = request
    const effectiveTimeout = timeoutMs ?? this.invocationTimeoutMs

    // Void is fire-and-forget — no invocation_id, no response
    if (action?.type === 'void') {
      const traceparent = injectTraceparent()
      const baggage = injectBaggage()
      this.sendMessage(MessageType.InvokeFunction, {
        function_id,
        data: payload,
        traceparent,
        baggage,
        action,
      })
      return undefined as TOutput
    }

    // Enqueue and default: send invocation_id, await response
    const invocation_id = crypto.randomUUID()
    const traceparent = injectTraceparent()
    const baggage = injectBaggage()

    return new Promise<TOutput>((resolve, reject) => {
      const timeout = setTimeout(() => {
        const invocation = this.invocations.get(invocation_id)
        if (invocation) {
          this.invocations.delete(invocation_id)
          reject(new Error(`Invocation timeout after ${effectiveTimeout}ms: ${function_id}`))
        }
      }, effectiveTimeout)

      this.invocations.set(invocation_id, {
        resolve: (result: TOutput) => {
          clearTimeout(timeout)
          resolve(result)
        },
        reject: (error: unknown) => {
          clearTimeout(timeout)
          reject(error)
        },
        timeout,
      })

      this.sendMessage(MessageType.InvokeFunction, {
        invocation_id,
        function_id,
        data: payload,
        traceparent,
        baggage,
        action,
      })
    })
  }

  /**
   * Lists all functions registered with the engine across all connected workers.
   *
   * @returns An array of {@link FunctionInfo} objects.
   *
   * @example
   * ```typescript
   * const functions = await iii.listFunctions()
   * functions.forEach(fn => console.log(fn.function_id))
   * ```
   */
  listFunctions = async (): Promise<FunctionInfo[]> => {
    const result = await this.trigger<Record<string, never>, { functions: FunctionInfo[] }>({
      function_id: EngineFunctions.LIST_FUNCTIONS,
      payload: {},
    })
    return result.functions
  }

  /**
   * Lists all connected workers.
   *
   * @returns An array of {@link WorkerInfo} objects.
   */
  listWorkers = async (): Promise<WorkerInfo[]> => {
    const result = await this.trigger<Record<string, never>, { workers: WorkerInfo[] }>({
      function_id: EngineFunctions.LIST_WORKERS,
      payload: {},
    })
    return result.workers
  }

  listTriggers = async (includeInternal = false): Promise<TriggerInfo[]> => {
    const result = await this.trigger<{ include_internal: boolean }, { triggers: TriggerInfo[] }>({
      function_id: EngineFunctions.LIST_TRIGGERS,
      payload: { include_internal: includeInternal },
    })
    return result.triggers
  }

  /**
   * Lists all trigger types registered with the engine.
   *
   * @param includeInternal - Whether to include internal trigger types (default: false).
   * @returns An array of {@link TriggerTypeInfo} objects.
   *
   * @example
   * ```typescript
   * const triggerTypes = await iii.listTriggerTypes()
   * triggerTypes.forEach(tt => console.log(`${tt.id}: ${tt.description}`))
   * ```
   */
  listTriggerTypes = async (includeInternal = false): Promise<TriggerTypeInfo[]> => {
    const result = await this.trigger<{ include_internal: boolean }, { trigger_types: TriggerTypeInfo[] }>({
      function_id: EngineFunctions.LIST_TRIGGER_TYPES,
      payload: { include_internal: includeInternal },
    })
    return result.trigger_types
  }

  private registerWorkerMetadata(): void {
    const telemetryOpts = this.options?.telemetry
    const language =
      telemetryOpts?.language ?? Intl.DateTimeFormat().resolvedOptions().locale ?? process.env.LANG?.split('.')[0]

    this.trigger({
      function_id: EngineFunctions.REGISTER_WORKER,
      payload: {
        runtime: 'node',
        version: SDK_VERSION,
        name: this.workerName,
        os: getOsInfo(),
        pid: process.pid,
        telemetry: {
          language,
          project_name: telemetryOpts?.project_name,
          framework: telemetryOpts?.framework,
          amplitude_api_key: telemetryOpts?.amplitude_api_key,
        },
      },
      action: { type: 'void' },
    })
  }

  /**
   * Registers a custom stream implementation, overriding the engine default
   * for the given stream name.
   *
   * Registers 5 of the 6 `IStream` methods (`get`, `set`, `delete`, `list`,
   * `listGroups`). The `update` method is not registered -- atomic updates are
   * handled by the engine's built-in stream update logic.
   *
   * @param streamName - Name of the stream.
   * @param stream - Object implementing the {@link IStream} interface.
   *
   * @example
   * ```typescript
   * iii.createStream('my-stream', {
   *   async get(input) { return null },
   *   async set(input) { return null },
   *   async delete(input) { return { old_value: undefined } },
   *   async list(input) { return [] },
   *   async listGroups(input) { return [] },
   *   async update(input) { return null },
   * })
   * ```
   */
  createStream = <TData>(streamName: string, stream: IStream<TData>): void => {
    this.registerFunction({ id: `stream::get(${streamName})` }, stream.get.bind(stream))
    this.registerFunction({ id: `stream::set(${streamName})` }, stream.set.bind(stream))
    this.registerFunction({ id: `stream::delete(${streamName})` }, stream.delete.bind(stream))
    this.registerFunction({ id: `stream::list(${streamName})` }, stream.list.bind(stream))
    this.registerFunction({ id: `stream::list_groups(${streamName})` }, stream.listGroups.bind(stream))
  }

  /**
   * Subscribes to function availability events from the engine. The callback
   * fires whenever the set of available functions changes.
   *
   * @param callback - Receives the current list of {@link FunctionInfo} objects.
   * @returns An unsubscribe function.
   *
   * @example
   * ```typescript
   * const unsub = iii.onFunctionsAvailable((functions) => {
   *   console.log('Available:', functions.map(f => f.function_id))
   * })
   *
   * // Later...
   * unsub()
   * ```
   */
  onFunctionsAvailable = (callback: FunctionsAvailableCallback): (() => void) => {
    this.functionsAvailableCallbacks.add(callback)

    if (!this.functionsAvailableTrigger) {
      if (!this.functionsAvailableFunctionPath) {
        this.functionsAvailableFunctionPath = `engine.on_functions_available.${crypto.randomUUID()}`
      }

      const function_id = this.functionsAvailableFunctionPath
      if (!this.functions.has(function_id)) {
        this.registerFunction({ id: function_id }, async ({ functions }: { functions: FunctionInfo[] }) => {
          this.functionsAvailableCallbacks.forEach((handler) => {
            handler(functions)
          })
          return null
        })
      }

      this.functionsAvailableTrigger = this.registerTrigger({
        type: EngineTriggers.FUNCTIONS_AVAILABLE,
        function_id,
        config: {},
      })
    }

    return () => {
      this.functionsAvailableCallbacks.delete(callback)
      if (this.functionsAvailableCallbacks.size === 0 && this.functionsAvailableTrigger) {
        this.functionsAvailableTrigger.unregister()
        this.functionsAvailableTrigger = undefined
      }
    }
  }

  /**
   * Gracefully shutdown the iii, cleaning up all resources.
   */
  shutdown = async (): Promise<void> => {
    this.isShuttingDown = true

    this.stopMetricsReporting()

    // Shutdown OpenTelemetry
    await shutdownOtel()

    // Clear reconnection timeout
    this.clearReconnectTimeout()

    // Reject all pending invocations
    for (const [_id, invocation] of this.invocations) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      invocation.reject(new Error('iii is shutting down'))
    }
    this.invocations.clear()

    // Close WebSocket
    if (this.ws) {
      this.ws.removeAllListeners()
      this.ws.close()
      this.ws = undefined
    }

    this.setConnectionState('disconnected')
  }

  // private methods

  private setConnectionState(state: IIIConnectionState): void {
    if (this.connectionState !== state) {
      this.connectionState = state
    }
  }

  private connect(): void {
    if (this.isShuttingDown) {
      return
    }

    this.setConnectionState('connecting')
    this.ws = new WebSocket(this.address)
    this.ws.on('open', this.onSocketOpen.bind(this))
    this.ws.on('close', this.onSocketClose.bind(this))
    this.ws.on('error', this.onSocketError.bind(this))
  }

  private clearReconnectTimeout(): void {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout)
      this.reconnectTimeout = undefined
    }
  }

  private scheduleReconnect(): void {
    if (this.isShuttingDown) {
      return
    }

    const { maxRetries, initialDelayMs, backoffMultiplier, maxDelayMs, jitterFactor } = this.reconnectionConfig

    if (maxRetries !== -1 && this.reconnectAttempt >= maxRetries) {
      this.setConnectionState('failed')
      this.logError(`Max reconnection retries (${maxRetries}) reached, giving up`)
      return
    }

    if (this.reconnectTimeout) {
      return // Already scheduled
    }

    const exponentialDelay = initialDelayMs * backoffMultiplier ** this.reconnectAttempt
    const cappedDelay = Math.min(exponentialDelay, maxDelayMs)
    const jitter = cappedDelay * jitterFactor * (2 * Math.random() - 1)
    const delay = Math.floor(cappedDelay + jitter)

    this.setConnectionState('reconnecting')
    console.debug(`[iii] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempt + 1})...`)

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectTimeout = undefined
      this.reconnectAttempt++
      this.connect()
    }, delay)
  }

  private onSocketError(error: Error): void {
    this.logError('WebSocket error', error)
  }

  private startMetricsReporting(): void {
    if (!this.metricsReportingEnabled || !this.workerId) {
      return
    }

    const meter = getMeter()
    if (!meter) {
      console.warn(
        '[iii] Worker metrics disabled: OpenTelemetry not initialized. Call initOtel() with metricsEnabled: true before creating the iii.',
      )
      return
    }

    registerWorkerGauges(meter, {
      workerId: this.workerId,
      workerName: this.workerName,
    })
  }

  private stopMetricsReporting(): void {
    stopWorkerGauges()
  }

  private onSocketClose(): void {
    this.ws?.removeAllListeners()
    this.ws?.terminate()
    this.ws = undefined

    this.setConnectionState('disconnected')
    this.stopMetricsReporting()
    this.scheduleReconnect()
  }

  private onSocketOpen(): void {
    this.clearReconnectTimeout()
    this.reconnectAttempt = 0
    this.setConnectionState('connected')

    this.ws?.on('message', this.onMessage.bind(this))

    this.triggerTypes.forEach(({ message }) => {
      this.sendMessage(MessageType.RegisterTriggerType, message, true)
    })
    this.services.forEach((service) => {
      this.sendMessage(MessageType.RegisterService, service, true)
    })
    this.functions.forEach(({ message }) => {
      this.sendMessage(MessageType.RegisterFunction, message, true)
    })
    this.triggers.forEach((trigger) => {
      this.sendMessage(MessageType.RegisterTrigger, trigger, true)
    })

    // Optimized: swap with empty array instead of splice
    const pending = this.messagesToSend
    this.messagesToSend = []
    for (const message of pending) {
      if (
        message.type === MessageType.InvokeFunction &&
        typeof message.invocation_id === 'string' &&
        !this.invocations.has(message.invocation_id)
      ) {
        continue
      }
      this.sendMessageRaw(JSON.stringify(message))
    }

    this.registerWorkerMetadata()
  }

  private isOpen(): boolean {
    return this.ws?.readyState === WebSocket.OPEN
  }

  private sendMessageRaw(data: string): void {
    if (this.ws && this.isOpen()) {
      try {
        this.ws.send(data, (err) => {
          if (err) {
            this.logError('Failed to send message', err)
          }
        })
      } catch (error) {
        this.logError('Exception while sending message', error)
      }
    }
  }

  private toWireFormat(messageType: MessageType, message: Omit<IIIMessage, 'message_type'>): Record<string, unknown> {
    const { message_type: _, ...rest } = message as Record<string, unknown>
    if (messageType === MessageType.RegisterTrigger && 'type' in message) {
      const { type: triggerType, ...triggerRest } = message as RegisterTriggerMessage
      return { type: messageType, ...triggerRest, trigger_type: triggerType }
    }
    if (messageType === MessageType.UnregisterTrigger && 'type' in message) {
      const { type: triggerType, ...triggerRest } = message as RegisterTriggerMessage
      return { type: messageType, ...triggerRest, trigger_type: triggerType }
    }
    if (messageType === MessageType.TriggerRegistrationResult && 'type' in message) {
      const { type: triggerType, ...resultRest } = message as TriggerRegistrationResultMessage
      return { type: messageType, ...resultRest, trigger_type: triggerType }
    }
    return { type: messageType, ...rest } as Record<string, unknown>
  }

  private sendMessage(messageType: MessageType, message: Omit<IIIMessage, 'message_type'>, skipIfClosed = false): void {
    const wireMessage = this.toWireFormat(messageType, message)
    if (this.isOpen()) {
      this.sendMessageRaw(JSON.stringify(wireMessage))
    } else if (!skipIfClosed) {
      this.messagesToSend.push(wireMessage)
    }
  }

  private logError(message: string, error?: unknown): void {
    const otelLogger = getLogger()
    const errorMessage = error instanceof Error ? error.message : String(error ?? '')

    if (otelLogger) {
      otelLogger.emit({
        severityNumber: SeverityNumber.ERROR,
        body: `[iii] ${message}${errorMessage ? `: ${errorMessage}` : ''}`,
      })
    } else {
      console.error(`[iii] ${message}`, error ?? '')
    }
  }

  private onInvocationResult(invocation_id: string, result: unknown, error: unknown): void {
    const invocation = this.invocations.get(invocation_id)

    if (invocation) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      error ? invocation.reject(error) : invocation.resolve(result)
    }

    this.invocations.delete(invocation_id)
  }

  private resolveChannelValue(value: unknown): unknown {
    if (isChannelRef(value)) {
      return value.direction === 'read'
        ? new ChannelReader(this.address, value)
        : new ChannelWriter(this.address, value)
    }
    if (Array.isArray(value)) {
      return value.map((item) => this.resolveChannelValue(item))
    }
    if (value !== null && typeof value === 'object') {
      const out: Record<string, unknown> = {}
      for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
        out[k] = this.resolveChannelValue(v)
      }
      return out
    }
    return value
  }

  private async onInvokeFunction<TInput>(
    invocation_id: string | undefined,
    function_id: string,
    input: TInput,
    traceparent?: string,
    baggage?: string,
  ): Promise<unknown> {
    const fn = this.functions.get(function_id)
    const getResponseTraceparent = () => injectTraceparent() ?? traceparent
    const getResponseBaggage = () => injectBaggage() ?? baggage

    const resolvedInput = this.resolveChannelValue(input) as TInput

    if (fn?.handler) {
      if (!invocation_id) {
        try {
          await fn.handler(resolvedInput, traceparent, baggage)
        } catch (error) {
          this.logError(`Error invoking function ${function_id}`, error)
        }
        return
      }

      try {
        const result = await fn.handler(resolvedInput, traceparent, baggage)
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_id,
          result,
          traceparent: getResponseTraceparent(),
          baggage: getResponseBaggage(),
        })
      } catch (error) {
        const isError = error instanceof Error
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_id,
          error: {
            code: 'invocation_failed',
            message: isError ? error.message : String(error),
            stacktrace: isError ? error.stack : undefined,
          },
          traceparent: getResponseTraceparent(),
          baggage: getResponseBaggage(),
        })
      }
    } else {
      const errorCode = fn ? 'function_not_invokable' : 'function_not_found'
      const errorMessage = fn ? 'Function is HTTP-invoked and cannot be invoked locally' : 'Function not found'
      if (invocation_id) {
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_id,
          error: { code: errorCode, message: errorMessage },
          traceparent,
          baggage,
        })
      }
    }
  }

  private async onRegisterTrigger(message: { trigger_type: string; id: string; function_id: string; config: unknown }) {
    const { trigger_type, id, function_id, config } = message
    const triggerTypeData = this.triggerTypes.get(trigger_type)

    if (triggerTypeData) {
      try {
        await triggerTypeData.handler.registerTrigger({ id, function_id, config })
        this.sendMessage(MessageType.TriggerRegistrationResult, {
          id,
          message_type: MessageType.TriggerRegistrationResult,
          type: trigger_type,
          function_id,
        })
      } catch (error) {
        this.sendMessage(MessageType.TriggerRegistrationResult, {
          id,
          message_type: MessageType.TriggerRegistrationResult,
          type: trigger_type,
          function_id,
          error: { code: 'trigger_registration_failed', message: (error as Error).message },
        })
      }
    } else {
      this.sendMessage(MessageType.TriggerRegistrationResult, {
        id,
        message_type: MessageType.TriggerRegistrationResult,
        type: trigger_type,
        function_id,
        error: { code: 'trigger_type_not_found', message: 'Trigger type not found' },
      })
    }
  }

  private onMessage(socketMessage: Data): void {
    let msgType: MessageType
    let message: Record<string, unknown>

    try {
      const parsed = JSON.parse(socketMessage.toString()) as Record<string, unknown>
      msgType = parsed.type as MessageType
      const { type: _, ...rest } = parsed
      message = rest
    } catch (error) {
      this.logError('Failed to parse incoming message', error)
      return
    }

    if (msgType === MessageType.InvocationResult) {
      const { invocation_id, result, error } = message as InvocationResultMessage
      this.onInvocationResult(invocation_id, result, error)
    } else if (msgType === MessageType.InvokeFunction) {
      const { invocation_id, function_id, data, traceparent, baggage } = message as InvokeFunctionMessage
      this.onInvokeFunction(invocation_id, function_id, data, traceparent, baggage)
    } else if (msgType === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(message as { trigger_type: string; id: string; function_id: string; config: unknown })
    } else if (msgType === MessageType.WorkerRegistered) {
      const { worker_id } = message as WorkerRegisteredMessage
      this.workerId = worker_id
      console.debug('[iii] Worker registered with ID:', worker_id)
      this.startMetricsReporting()
    }
  }
}

/**
 * Factory object that constructs routing actions for {@link ISdk.trigger}.
 *
 * @example
 * ```typescript
 * import { TriggerAction } from 'iii-sdk'
 *
 * // Enqueue to a named queue
 * iii.trigger({
 *   function_id: 'process',
 *   payload: { data: 'hello' },
 *   action: TriggerAction.Enqueue({ queue: 'jobs' }),
 * })
 *
 * // Fire-and-forget
 * iii.trigger({
 *   function_id: 'notify',
 *   payload: {},
 *   action: TriggerAction.Void(),
 * })
 * ```
 */
export const TriggerAction = {
  /**
   * Routes the invocation through a named queue. The engine enqueues the job,
   * acknowledges the caller with `{ messageReceiptId }`, and processes it
   * asynchronously.
   *
   * @param opts - Queue routing options.
   * @param opts.queue - Name of the target queue.
   */
  Enqueue: (opts: { queue: string }): TriggerActionType => ({ type: 'enqueue', ...opts }),
  /**
   * Fire-and-forget routing. The engine forwards the invocation without
   * waiting for a response or queuing the job.
   */
  Void: (): TriggerActionType => ({ type: 'void' }),
} as const

/**
 * Creates and returns a connected SDK instance. The WebSocket connection is
 * established automatically -- there is no separate `connect()` call.
 *
 * @param address - WebSocket URL of the III engine (e.g. `ws://localhost:49134`).
 * @param options - Optional {@link InitOptions} for worker name, timeouts, reconnection, and OTel.
 * @returns A connected {@link ISdk} instance.
 *
 * @example
 * ```typescript
 * import { registerWorker } from 'iii-sdk'
 *
 * const iii = registerWorker(process.env.III_URL ?? 'ws://localhost:49134', {
 *   workerName: 'my-worker',
 * })
 * ```
 */
export const registerWorker = (address: string, options?: InitOptions): ISdk => new Sdk(address, options)
