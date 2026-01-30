import { context } from '@opentelemetry/api'
import { createRequire } from 'module'
import * as os from 'os'
import { type Data, WebSocket } from 'ws'
import {
  type BridgeConnectionState,
  type BridgeReconnectionConfig,
  DEFAULT_BRIDGE_RECONNECTION_CONFIG,
  DEFAULT_INVOCATION_TIMEOUT_MS,
  EngineFunctions,
  EngineTriggers,
} from './bridge-constants'
import {
  type BridgeMessage,
  type FunctionInfo,
  type InvocationResultMessage,
  type InvokeFunctionMessage,
  MessageType,
  type RegisterFunctionMessage,
  type RegisterServiceMessage,
  type RegisterTriggerMessage,
  type RegisterTriggerTypeMessage,
  type WorkerInfo,
  type WorkerRegisteredMessage,
} from './bridge-types'
import { withContext } from './context'
import { Logger } from './logger'
import type { IStream } from './streams'
import {
  currentSpanId,
  currentTraceId,
  extractContext,
  getLogger,
  getMeter,
  getTracer,
  injectBaggage,
  injectTraceparent,
  initOtel,
  shutdownOtel,
  SeverityNumber,
  SpanKind,
  withSpan,
  type OtelConfig,
} from './telemetry'
import { registerWorkerGauges, stopWorkerGauges } from './otel-worker-gauges'
import type { TriggerHandler } from './triggers'
import type {
  BridgeClient,
  FunctionsAvailableCallback,
  Invocation,
  RemoteFunctionData,
  RemoteFunctionHandler,
  RemoteTriggerTypeData,
  Trigger,
} from './types'

const require = createRequire(import.meta.url)
const { version: SDK_VERSION } = require('../package.json')

function getOsInfo(): string {
  return `${os.platform()} ${os.release()} (${os.arch()})`
}

function getDefaultWorkerName(): string {
  return `${os.hostname()}:${process.pid}`
}

/** Callback type for connection state changes */
export type ConnectionStateCallback = (state: BridgeConnectionState) => void

export type BridgeOptions = {
  workerName?: string
  enableMetricsReporting?: boolean
  /** Default timeout for function invocations in milliseconds */
  invocationTimeoutMs?: number
  /** Configuration for WebSocket reconnection behavior */
  reconnectionConfig?: Partial<BridgeReconnectionConfig>
  /** OpenTelemetry configuration. If provided, OTEL will be initialized automatically.
   * The engineWsUrl is set automatically from the Bridge address. */
  otel?: Omit<OtelConfig, 'engineWsUrl'>
}

export class Bridge implements BridgeClient {
  private ws?: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private services = new Map<string, Omit<RegisterServiceMessage, 'functions'>>()
  private invocations = new Map<string, Invocation & { timeout?: NodeJS.Timeout }>()
  private triggers = new Map<string, RegisterTriggerMessage>()
  private triggerTypes = new Map<string, RemoteTriggerTypeData>()
  private functionsAvailableCallbacks = new Set<FunctionsAvailableCallback>()
  private functionsAvailableTrigger?: Trigger
  private functionsAvailableFunctionPath?: string
  private messagesToSend: BridgeMessage[] = []
  private workerName: string
  private workerId?: string
  private reconnectTimeout?: NodeJS.Timeout
  private metricsReportingEnabled: boolean
  private invocationTimeoutMs: number
  private reconnectionConfig: BridgeReconnectionConfig
  private reconnectAttempt = 0
  private connectionState: BridgeConnectionState = 'disconnected'
  private stateCallbacks = new Set<ConnectionStateCallback>()
  private isShuttingDown = false

  constructor(
    private readonly address: string,
    options?: BridgeOptions,
  ) {
    this.workerName = options?.workerName ?? getDefaultWorkerName()
    this.metricsReportingEnabled = options?.enableMetricsReporting ?? true
    this.invocationTimeoutMs = options?.invocationTimeoutMs ?? DEFAULT_INVOCATION_TIMEOUT_MS
    this.reconnectionConfig = { ...DEFAULT_BRIDGE_RECONNECTION_CONFIG, ...options?.reconnectionConfig }

    // Initialize OpenTelemetry if config is provided
    if (options?.otel) {
      initOtel({ ...options.otel, engineWsUrl: this.address })
    }

    this.connect()
  }

  registerTriggerType<TConfig>(
    triggerType: Omit<RegisterTriggerTypeMessage, 'type'>,
    handler: TriggerHandler<TConfig>,
  ): void {
    this.sendMessage(MessageType.RegisterTriggerType, triggerType, true)
    this.triggerTypes.set(triggerType.id, {
      message: { ...triggerType, type: MessageType.RegisterTriggerType },
      handler,
    })
  }

  on(event: string, callback: (arg?: unknown) => void): void {
    this.ws?.on(event, callback)
  }

  unregisterTriggerType(triggerType: Omit<RegisterTriggerTypeMessage, 'type'>): void {
    this.sendMessage(MessageType.UnregisterTriggerType, triggerType, true)
    this.triggerTypes.delete(triggerType.id)
  }

  registerTrigger(trigger: Omit<RegisterTriggerMessage, 'type' | 'id'>): Trigger {
    const id = crypto.randomUUID()
    this.sendMessage(MessageType.RegisterTrigger, { ...trigger, id }, true)
    this.triggers.set(id, { ...trigger, id, type: MessageType.RegisterTrigger })

    return {
      unregister: () => {
        this.sendMessage(MessageType.UnregisterTrigger, { id, type: MessageType.UnregisterTrigger })
        this.triggers.delete(id)
      },
    }
  }

  registerFunction(message: Omit<RegisterFunctionMessage, 'type'>, handler: RemoteFunctionHandler): void {
    if (!message.function_path || message.function_path.trim() === '') {
      throw new Error('function_path is required')
    }

    this.sendMessage(MessageType.RegisterFunction, message, true)
    this.functions.set(message.function_path, {
      message: { ...message, type: MessageType.RegisterFunction },
      handler: async (input, traceparent?: string, baggage?: string) => {
        // If we have a tracer, wrap in a span and pass it to the context
        if (getTracer()) {
          // Extract both traceparent and baggage into a parent context
          const parentContext = extractContext(traceparent, baggage)
          
          return context.with(parentContext, () =>
            withSpan(`invoke ${message.function_path}`, { kind: SpanKind.SERVER }, async (span) => {
              const traceId = currentTraceId() ?? crypto.randomUUID()
              const spanId = currentSpanId()
              const logger = new Logger(undefined, traceId, message.function_path, spanId)
              const ctx = { logger, trace: span }

              return withContext(async () => await handler(input), ctx)
            })
          )
        }

        // Fallback without tracing
        const traceId = crypto.randomUUID()
        const logger = new Logger(undefined, traceId, message.function_path)
        const ctx = { logger }

        return withContext(async () => await handler(input), ctx)
      },
    })
  }

  registerService(message: Omit<RegisterServiceMessage, 'type'>): void {
    this.sendMessage(MessageType.RegisterService, message, true)
    this.services.set(message.id, { ...message, type: MessageType.RegisterService })
  }

  async invokeFunction<TInput, TOutput>(function_path: string, data: TInput, timeoutMs?: number): Promise<TOutput> {
    const invocation_id = crypto.randomUUID()
    // Inject trace context and baggage if available
    const traceparent = injectTraceparent()
    const baggage = injectBaggage()
    const effectiveTimeout = timeoutMs ?? this.invocationTimeoutMs

    return new Promise<TOutput>((resolve, reject) => {
      const timeout = setTimeout(() => {
        const invocation = this.invocations.get(invocation_id)
        if (invocation) {
          this.invocations.delete(invocation_id)
          reject(new Error(`Invocation timeout after ${effectiveTimeout}ms: ${function_path}`))
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

      this.sendMessage(MessageType.InvokeFunction, { invocation_id, function_path, data, traceparent, baggage })
    })
  }

  invokeFunctionAsync<TInput>(function_path: string, data: TInput): void {
    // Inject trace context and baggage if available
    const traceparent = injectTraceparent()
    const baggage = injectBaggage()
    this.sendMessage(MessageType.InvokeFunction, { function_path, data, traceparent, baggage })
  }

  async listFunctions(): Promise<FunctionInfo[]> {
    const result = await this.invokeFunction<Record<string, never>, { functions: FunctionInfo[] }>(
      EngineFunctions.LIST_FUNCTIONS,
      {},
    )
    return result.functions
  }

  async listWorkers(): Promise<WorkerInfo[]> {
    const result = await this.invokeFunction<Record<string, never>, { workers: WorkerInfo[] }>(
      EngineFunctions.LIST_WORKERS,
      {},
    )
    return result.workers
  }

  private registerWorkerMetadata(): void {
    this.invokeFunctionAsync(EngineFunctions.REGISTER_WORKER, {
      runtime: 'node',
      version: SDK_VERSION,
      name: this.workerName,
      os: getOsInfo(),
    })
  }

  createStream<TData>(streamName: string, stream: IStream<TData>): void {
    this.registerFunction({ function_path: `streams.get(${streamName})` }, stream.get.bind(stream))
    this.registerFunction({ function_path: `streams.set(${streamName})` }, stream.set.bind(stream))
    this.registerFunction({ function_path: `streams.delete(${streamName})` }, stream.delete.bind(stream))
    this.registerFunction({ function_path: `streams.getGroup(${streamName})` }, stream.getGroup.bind(stream))
    this.registerFunction({ function_path: `streams.listGroups(${streamName})` }, stream.listGroups.bind(stream))
  }

  onFunctionsAvailable(callback: FunctionsAvailableCallback): () => void {
    this.functionsAvailableCallbacks.add(callback)

    if (!this.functionsAvailableTrigger) {
      if (!this.functionsAvailableFunctionPath) {
        this.functionsAvailableFunctionPath = `bridge.on_functions_available.${crypto.randomUUID()}`
      }

      const function_path = this.functionsAvailableFunctionPath
      if (!this.functions.has(function_path)) {
        this.registerFunction({ function_path }, async ({functions}: { functions: FunctionInfo[] }) => {
          this.functionsAvailableCallbacks.forEach((handler) => {
            handler(functions)
          })
          return null
        })
      }

      this.functionsAvailableTrigger = this.registerTrigger({
        trigger_type: EngineTriggers.FUNCTIONS_AVAILABLE,
        function_path,
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
   * Get the current connection state.
   */
  getConnectionState(): BridgeConnectionState {
    return this.connectionState
  }

  /**
   * Register a callback to be notified of connection state changes.
   * @returns A function to unregister the callback
   */
  onConnectionStateChange(callback: ConnectionStateCallback): () => void {
    this.stateCallbacks.add(callback)
    // Immediately notify of current state
    callback(this.connectionState)
    return () => this.stateCallbacks.delete(callback)
  }

  /**
   * Gracefully shutdown the bridge, cleaning up all resources.
   */
  async shutdown(): Promise<void> {
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
      invocation.reject(new Error('Bridge shutting down'))
    }
    this.invocations.clear()
    
    // Close WebSocket
    if (this.ws) {
      this.ws.removeAllListeners()
      this.ws.close()
      this.ws = undefined
    }
    
    // Clear callbacks
    this.stateCallbacks.clear()
    
    this.setConnectionState('disconnected')
  }

  // private methods

  private setConnectionState(state: BridgeConnectionState): void {
    if (this.connectionState !== state) {
      this.connectionState = state
      for (const callback of this.stateCallbacks) {
        try {
          callback(state)
        } catch (error) {
          this.logError('Error in connection state callback', error)
        }
      }
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
    
    const exponentialDelay = initialDelayMs * Math.pow(backoffMultiplier, this.reconnectAttempt)
    const cappedDelay = Math.min(exponentialDelay, maxDelayMs)
    const jitter = cappedDelay * jitterFactor * (2 * Math.random() - 1)
    const delay = Math.floor(cappedDelay + jitter)
    
    this.setConnectionState('reconnecting')
    console.debug(`[Bridge] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempt + 1})...`)
    
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
      console.warn('[Bridge] Worker metrics disabled: OpenTelemetry not initialized. Call initOtel() with metricsEnabled: true before creating the Bridge.')
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
      // Skip InvokeFunction messages for timed-out invocations
      if (
        message.type === MessageType.InvokeFunction &&
        message.invocation_id &&
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

  private sendMessage(type: MessageType, message: Omit<BridgeMessage, 'type'>, skipIfClosed = false): void {
    const fullMessage = { ...message, type }
    if (this.isOpen()) {
      this.sendMessageRaw(JSON.stringify(fullMessage))
    } else if (!skipIfClosed) {
      this.messagesToSend.push(fullMessage as BridgeMessage)
    }
  }

  private logError(message: string, error?: unknown): void {
    const otelLogger = getLogger()
    const errorMessage = error instanceof Error ? error.message : String(error ?? '')
    
    if (otelLogger) {
      otelLogger.emit({
        severityNumber: SeverityNumber.ERROR,
        body: `[Bridge] ${message}${errorMessage ? `: ${errorMessage}` : ''}`,
      })
    } else {
      console.error(`[Bridge] ${message}`, error ?? '')
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

  private async onInvokeFunction<TInput>(
    invocation_id: string | undefined,
    function_path: string,
    input: TInput,
    traceparent?: string,
    baggage?: string,
  ): Promise<unknown> {
    const fn = this.functions.get(function_path)
    // Get response traceparent/baggage after handler runs (will be current span's context)
    const getResponseTraceparent = () => injectTraceparent() ?? traceparent
    const getResponseBaggage = () => injectBaggage() ?? baggage

    if (fn) {
      if (!invocation_id) {
        try {
          await fn.handler(input, traceparent, baggage)
        } catch (error) {
          this.logError(`Error invoking function ${function_path}`, error)
        }
        return
      }

      try {
        const result = await fn.handler(input, traceparent, baggage)
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_path,
          result,
          traceparent: getResponseTraceparent(),
          baggage: getResponseBaggage(),
        })
      } catch (error) {
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_path,
          error: { code: 'invocation_failed', message: (error as Error).message },
          traceparent: getResponseTraceparent(),
          baggage: getResponseBaggage(),
        })
      }
    } else {
      this.sendMessage(MessageType.InvocationResult, {
        invocation_id,
        function_path,
        error: { code: 'function_not_found', message: 'Function not found' },
        traceparent,
        baggage,
      })
    }
  }

  private async onRegisterTrigger(message: RegisterTriggerMessage) {
    const triggerTypeData = this.triggerTypes.get(message.trigger_type)
    const { id, trigger_type, function_path, config } = message

    if (triggerTypeData) {
      try {
        await triggerTypeData.handler.registerTrigger({ id, function_path, config })
        this.sendMessage(MessageType.TriggerRegistrationResult, { id, trigger_type, function_path })
      } catch (error) {
        this.sendMessage(MessageType.TriggerRegistrationResult, {
          id,
          trigger_type,
          function_path,
          error: { code: 'trigger_registration_failed', message: (error as Error).message },
        })
      }
    } else {
      this.sendMessage(MessageType.TriggerRegistrationResult, {
        id,
        trigger_type,
        function_path,
        error: { code: 'trigger_type_not_found', message: 'Trigger type not found' },
      })
    }
  }

  private onMessage(socketMessage: Data): void {
    let type: MessageType
    let message: Omit<BridgeMessage, 'type'>
    
    try {
      const parsed = JSON.parse(socketMessage.toString()) as BridgeMessage
      type = parsed.type
      const { type: _, ...rest } = parsed
      message = rest
    } catch (error) {
      this.logError('Failed to parse incoming message', error)
      return
    }

    if (type === MessageType.InvocationResult) {
      const { invocation_id, result, error } = message as InvocationResultMessage
      this.onInvocationResult(invocation_id, result, error)
    } else if (type === MessageType.InvokeFunction) {
      const { invocation_id, function_path, data, traceparent, baggage } = message as InvokeFunctionMessage
      this.onInvokeFunction(invocation_id, function_path, data, traceparent, baggage)
    } else if (type === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(message as RegisterTriggerMessage)
    } else if (type === MessageType.WorkerRegistered) {
      const { worker_id } = message as WorkerRegisteredMessage
      this.workerId = worker_id
      console.debug('[Bridge] Worker registered with ID:', worker_id)
      this.startMetricsReporting()
    }
  }
}
