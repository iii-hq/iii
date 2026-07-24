import { context, trace } from '@opentelemetry/api'
import { createRequire } from 'node:module'
import * as os from 'node:os'
import { type Data, WebSocket } from 'ws'
import { ChannelReader, ChannelWriter } from './channels'
import { InvocationError, isErrorBody, RegistrationRejectedError } from './errors'
import {
  DEFAULT_BRIDGE_RECONNECTION_CONFIG,
  DEFAULT_INVOCATION_TIMEOUT_MS,
  EngineFunctions,
  type IIIConnectionState,
  type IIIReconnectionConfig,
  WS_HANDSHAKE_TIMEOUT_MS,
  WS_IDLE_TIMEOUT_MS,
  WS_PING_INTERVAL_MS,
} from './iii-constants'
import type { HttpInvocationConfig } from '@iii-dev/helpers/http'
import {
  type IIIMessage,
  type InvocationResultMessage,
  type InvokeFunctionMessage,
  type JsonValue,
  MessageType,
  type RegisterFunctionMessage,
  type RegistrationRejectedMessage,
  type RegisterTriggerMessage,
  type RegisterTriggerTypeMessage,
  type StreamChannelRef,
  type TriggerRegistrationResultMessage,
  type TriggerRequest,
  type WorkerRegisteredMessage,
} from './iii-types'
import { registerWorkerGauges, stopWorkerGauges } from '@iii-dev/helpers/observability'
import { getMeter, getTracer } from '@iii-dev/helpers/observability/internal'
import { SpanKind } from '@opentelemetry/api'
import type { IStream } from './stream'
import { detectProjectName } from './utils'
import {
  extractContext,
  getLogger,
  initOtel,
  injectBaggage,
  injectTraceparent,
  type OtelConfig,
  recordSpanEvent,
  redactAndTruncate,
  resolveMaxBytesFromEnv,
  SeverityNumber,
  shutdownOtel,
  withSpan,
} from '@iii-dev/helpers/observability'
import type { TriggerHandler } from './triggers'
import type {
  FunctionRef,
  IIIClient,
  Invocation,
  RegisterFunctionOptions,
  RemoteFunctionData,
  RemoteFunctionHandler,
  RemoteTriggerTypeData,
  Trigger,
  TriggerTypeRef,
} from './types'
import { isChannelRef } from './utils'

const require = createRequire(import.meta.url)
const { version: SDK_VERSION } = require('../package.json')

/**
 * `registrationrejected` code (engine `protocol.rs`): another live worker
 * already holds this `(namespace, worker_name)`. The engine closes the
 * connection -- fatal.
 */
const WORKER_NAMESPACE_CONFLICT = 'WORKER_NAMESPACE_CONFLICT'

/**
 * `registrationrejected` code (engine `protocol.rs`): another live worker in
 * this namespace already exports this one function id. The connection stays
 * open -- non-fatal.
 */
const FUNCTION_NAMESPACE_CONFLICT = 'FUNCTION_NAMESPACE_CONFLICT'

function getOsInfo(): string {
  return `${os.platform()} ${os.release()} (${os.arch()})`
}

function getDefaultWorkerName(): string {
  // III_WORKER_NAME carries the config.yaml entry name for managed workers
  // (set by iii-worker at spawn). Engine truth (`iii worker status`/`list`)
  // matches connections by name, so the managed identity must win over the
  // hostname:pid fallback.
  const managedName = process.env.III_WORKER_NAME
  if (managedName) {
    return managedName
  }
  return `${os.hostname()}:${process.pid}`
}

function resolveNamespace(optionNamespace?: string): string | undefined {
  // III_NAMESPACE carries the namespace for managed workers (set by iii-worker
  // at spawn), mirroring III_WORKER_NAME. An explicit option wins; otherwise
  // the env var provides the managed identity. Absent in both means undefined
  // -- the engine applies its `default` namespace when none is on the wire.
  if (optionNamespace) {
    return optionNamespace
  }
  const managedNamespace = process.env.III_NAMESPACE
  if (managedNamespace) {
    return managedNamespace
  }
  return undefined
}

/** Worker metadata reported to the engine (language, framework, project). */
export type TelemetryOptions = {
  /** Programming language of the worker. */
  language?: string
  /** Name of the project this worker belongs to. */
  project_name?: string
  /** Framework name, if applicable. */
  framework?: string
  /** Amplitude API key for product analytics. */
  amplitude_api_key?: string
}

/**
 * Configuration options passed to {@link registerWorker}.
 *
 * @example
 * ```typescript
 * const worker = registerWorker('ws://localhost:49134', {
 *   workerName: 'my-worker',
 *   invocationTimeoutMs: 10000,
 *   reconnectionConfig: { maxRetries: 5 },
 * })
 * ```
 */
export type InitOptions = {
  /** Display name for this worker. Defaults to `hostname:pid`. */
  workerName?: string
  /**
   * Namespace this worker registers under. Resolution order:
   * `options.namespace` -> `process.env.III_NAMESPACE` -> undefined. When
   * undefined the engine applies its `default` namespace. Scopes worker and
   * function registrations so identically-named entries can coexist across
   * namespaces.
   */
  namespace?: string
  /**
   * One-line, human/LLM-readable summary of what this worker does.
   * Surfaces in `engine::workers::list` / `engine::workers::info`.
   */
  workerDescription?: string
  /** Enable worker metrics via OpenTelemetry. Defaults to `true`. */
  enableMetricsReporting?: boolean
  /** Default timeout for `worker.trigger()` invocations in milliseconds. Defaults to `30000`. */
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
  /** Custom HTTP headers sent during the WebSocket handshake. */
  headers?: Record<string, string>
  /** @internal */
  telemetry?: TelemetryOptions
}

class Sdk implements IIIClient {
  private ws?: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private invocations = new Map<string, Invocation & { timeout?: NodeJS.Timeout }>()
  private triggers = new Map<string, RegisterTriggerMessage>()
  private triggerTypes = new Map<string, RemoteTriggerTypeData>()
  private messagesToSend: Record<string, unknown>[] = []
  private workerName: string
  private namespace?: string
  private workerDescription?: string
  private workerId?: string
  private reattachToken?: string
  private reconnectTimeout?: NodeJS.Timeout
  private heartbeatInterval?: NodeJS.Timeout
  private idleTimeout?: NodeJS.Timeout
  private metricsReportingEnabled: boolean
  private invocationTimeoutMs: number
  private reconnectionConfig: IIIReconnectionConfig
  private reconnectAttempt = 0
  private connectionState: IIIConnectionState = 'disconnected'
  private isShuttingDown = false
  // Set when the engine fatally rejects this worker's registration (e.g. a
  // namespace/name collision). Terminal: no reconnect follows. Mirrors the
  // Python (`_fatal_error`) and Rust (`fatal_error()`) SDKs.
  private fatalError?: RegistrationRejectedError

  constructor(
    private readonly address: string,
    private readonly options?: InitOptions,
  ) {
    this.workerName = options?.workerName ?? getDefaultWorkerName()
    this.namespace = resolveNamespace(options?.namespace)
    this.workerDescription = options?.workerDescription
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
   * worker.registerTriggerType(
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
      // This typed helper pairs a function with its trigger, so it defaults the
      // trigger's namespace to this worker's — otherwise the function would land
      // in the worker's namespace and the trigger in `default`, and never resolve
      // it. The low-level `registerTrigger` keeps the engine default (`default`).
      registerTrigger: (functionId: string, config: TConfig, metadata?: Record<string, unknown>) => {
        return this.registerTrigger({
          type: triggerType.id,
          function_id: functionId,
          config,
          metadata,
          namespace: this.namespace,
        })
      },
      registerFunction: (functionId, handler, config, metadata?) => {
        const ref = this.registerFunction(functionId, handler)
        this.registerTrigger({
          type: triggerType.id,
          function_id: functionId,
          config,
          metadata,
          namespace: this.namespace,
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
   * @param trigger.type - Trigger type (e.g. `http`, `durable:subscriber`, `cron`).
   * @param trigger.function_id - ID of the function to invoke.
   * @param trigger.config - Trigger-specific configuration.
   * @returns A {@link Trigger} handle with an `unregister()` method.
   *
   * @example
   * ```typescript
   * const trigger = worker.registerTrigger({
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
   * Registers a function with the engine. The `functionId` is the unique identifier
   * used by triggers and invocations.
   *
   * Pass a handler for local execution, or an {@link HttpInvocationConfig}
   * for HTTP-invoked functions (Lambda, Cloudflare Workers, etc.).
   *
   * @param functionId - Unique function identifier.
   * @param handlerOrInvocation - Async handler or HTTP invocation config.
   * @param options - Optional function registration options (description, request/response formats, metadata).
   * @returns A {@link FunctionRef} with `id` and `unregister()`.
   *
   * @example
   * ```typescript
   * const fn = worker.registerFunction(
   *   'greet',
   *   async (input: { name: string }) => {
   *     return { message: `Hello, ${input.name}!` }
   *   },
   *   { description: 'Greets a user' },
   * )
   * ```
   */
  registerFunction = (
    functionId: string,
    handlerOrInvocation: RemoteFunctionHandler | HttpInvocationConfig,
    options?: RegisterFunctionOptions,
  ): FunctionRef => {
    if (!functionId || functionId.trim() === '') {
      throw new Error('id is required')
    }
    if (this.functions.has(functionId)) {
      throw new Error(`function id already registered: ${functionId}`)
    }

    const isHandler = typeof handlerOrInvocation === 'function'

    const fullMessage: RegisterFunctionMessage = isHandler
      ? { ...options, id: functionId, message_type: MessageType.RegisterFunction }
      : {
          ...options,
          id: functionId,
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
      this.functions.set(functionId, {
        message: fullMessage,
        handler: async (input, metadata?: JsonValue, traceparent?: string, baggage?: string) => {
          const tracePayloads = !(
            process.env.III_DISABLE_TRACE_PAYLOADS === '1' ||
            process.env.III_DISABLE_TRACE_PAYLOADS?.toLowerCase() === 'true'
          )
          const payloadMaxBytes = resolveMaxBytesFromEnv()

          const runHandler = async () => {
            if (tracePayloads) {
              const { json, truncated } = redactAndTruncate(input, payloadMaxBytes)
              recordSpanEvent('iii.invocation.input', {
                'iii.payload.json': json,
                'iii.payload.truncated': truncated,
              })
            }
            try {
              const result = await handler(input, metadata)
              if (tracePayloads) {
                const { json, truncated } = redactAndTruncate(result, payloadMaxBytes)
                recordSpanEvent('iii.invocation.output', {
                  'iii.payload.json': json,
                  'iii.payload.truncated': truncated,
                  'iii.payload.ok': true,
                })
              }
              return result
            } catch (err) {
              if (tracePayloads) {
                const errMsg = err instanceof Error ? err.message : String(err)
                const { json, truncated } = redactAndTruncate(
                  { error: errMsg },
                  payloadMaxBytes,
                )
                recordSpanEvent('iii.invocation.output', {
                  'iii.payload.json': json,
                  'iii.payload.truncated': truncated,
                  'iii.payload.ok': false,
                })
              }
              throw err
            }
          }

          if (getTracer()) {
            const parentContext = extractContext(traceparent, baggage)

            // INTERNAL and named `execute` (not `call`/`trigger`): the engine
            // suppresses its own `call <fn>` span for worker-routed functions
            // (this span is the canonical one for the invocation) but still
            // emits engine-side wrappers like `trigger <fn>` from fire_triggers
            // and `call <fn>` for builtins. Reusing those names would read as
            // duplicate engine spans under the worker's service. `execute` is
            // unique, so the worker handler span reads as a clean internal
            // child (and is collapsible by a single rule).
            return context.with(parentContext, () =>
              withSpan(`execute ${functionId}`, { kind: SpanKind.INTERNAL }, async () => await runHandler()),
            )
          }

          const traceId = crypto.randomUUID().replace(/-/g, '')
          const spanId = crypto.randomUUID().replace(/-/g, '').slice(0, 16)
          const syntheticSpan = trace.wrapSpanContext({ traceId, spanId, traceFlags: 1 })

          return context.with(trace.setSpan(context.active(), syntheticSpan), async () => await runHandler())
        },
      })
    } else {
      this.functions.set(functionId, { message: fullMessage })
    }

    return {
      id: functionId,
      unregister: () => {
        this.sendMessage(MessageType.UnregisterFunction, { id: functionId }, true)
        this.functions.delete(functionId)
      },
    }
  }

  /**
   * @internal Implementation backing the `createChannel` helper in the
   * `iii-sdk/helpers` submodule. Not part of the public `IIIClient` surface.
   *
   * Creates a streaming channel pair for worker-to-worker data transfer.
   * Returns a {@link Channel} with a local writer/reader and serializable refs
   * that can be passed as fields in invocation data to other functions.
   */
  __helpers_create_channel = async (bufferSize?: number): Promise<import('./types').Channel> => {
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
   * | _(none)_                      | Synchronous: waits for the function to return     | `Promise<TOutput>`       |
   * | `TriggerAction.Enqueue(...)` | Async via named queue; engine acknowledges enqueue | `Promise<EnqueueResult>` |
   * | `TriggerAction.Void()`       | Fire-and-forget, no response                      | `Promise<undefined>`     |
   *
   * @param request - The trigger request.
   * @param request.function_id - ID of the function to invoke.
   * @param request.payload - Payload to pass to the function.
   * @param request.action - Routing action. Omit for synchronous request/response.
   * @param request.timeoutMs - Override the default invocation timeout.
   * @param request.metadata - Optional per-invocation metadata (arbitrary JSON)
   *   surfaced to the target handler as its second argument.
   * @returns The result of the function invocation.
   *
   * @example
   * ```typescript
   * import { TriggerAction } from 'iii-sdk'
   *
   * // Synchronous
   * const result = await worker.trigger({ function_id: 'get-order', payload: { id: '123' } })
   *
   * // Enqueue
   * const { messageReceiptId } = await worker.trigger({
   *   function_id: 'payments::charge',
   *   payload: { orderId: '123', amount: 49.99 },
   *   action: TriggerAction.Enqueue({ queue: 'payment' }),
   * })
   *
   * // Fire-and-forget
   * worker.trigger({
   *   function_id: 'notifications::send',
   *   payload: { userId: '123' },
   *   action: TriggerAction.Void(),
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  trigger = async <TInput = unknown, TOutput = any>(
    request: TriggerRequest<TInput>,
  ): Promise<TOutput> => {
    const { function_id, payload, action, timeoutMs, metadata, namespace } = request
    const effectiveTimeout = timeoutMs ?? this.invocationTimeoutMs

    // Void is fire-and-forget, no invocation_id, no response
    if (action?.type === 'void') {
      const traceparent = injectTraceparent()
      const baggage = injectBaggage()
      this.sendMessage(MessageType.InvokeFunction, {
        function_id,
        data: payload,
        traceparent,
        baggage,
        action,
        metadata,
        // Omit when absent so the engine routes within its default namespace.
        ...(namespace !== undefined ? { namespace } : {}),
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
          reject(
            new InvocationError({
              code: 'TIMEOUT',
              message: `invocation timed out after ${effectiveTimeout}ms`,
              function_id,
            }),
          )
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
        function_id,
        timeout,
      })

      this.sendMessage(MessageType.InvokeFunction, {
        invocation_id,
        function_id,
        data: payload,
        traceparent,
        baggage,
        action,
        metadata,
        // Omit when absent so the engine routes within its default namespace.
        ...(namespace !== undefined ? { namespace } : {}),
      })
    })
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
        description: this.workerDescription,
        os: getOsInfo(),
        pid: process.pid,
        isolation: process.env.III_ISOLATION || null,
        // Omit when absent so the engine falls back to its `default` namespace.
        ...(this.namespace !== undefined ? { namespace: this.namespace } : {}),
        telemetry: {
          language,
          project_name: telemetryOpts?.project_name ?? detectProjectName(),
          framework: telemetryOpts?.framework?.trim() || 'iii-node',
          amplitude_api_key: telemetryOpts?.amplitude_api_key,
        },
      },
      action: { type: 'void' },
    })
  }

  /**
   * @internal Implementation backing the `createStream` helper in the
   * `iii-sdk/helpers` submodule. Not part of the public `IIIClient` surface.
   *
   * Registers a custom stream implementation, overriding the engine default
   * for the given stream name. Registers 5 of the 6 `IStream` methods
   * (`get`, `set`, `delete`, `list`, `listGroups`). The `update` method is
   * not registered; atomic updates are handled by the engine's built-in
   * stream update logic.
   */
  __helpers_create_stream = <TData>(streamName: string, stream: IStream<TData>): void => {
    this.registerFunction(`stream::get(${streamName})`, stream.get.bind(stream))
    this.registerFunction(`stream::set(${streamName})`, stream.set.bind(stream))
    this.registerFunction(`stream::delete(${streamName})`, stream.delete.bind(stream))
    this.registerFunction(`stream::list(${streamName})`, stream.list.bind(stream))
    this.registerFunction(`stream::list_groups(${streamName})`, stream.listGroups.bind(stream))
  }

  /**
   * The current WebSocket connection state. `'failed'` is terminal — it follows
   * a fatal registration rejection (see {@link getFatalError}). Mirrors the
   * Python/Rust SDKs' `get_connection_state()`.
   */
  getConnectionState = (): IIIConnectionState => this.connectionState

  /**
   * The fatal registration rejection that terminated this connection, if any
   * (e.g. a `WORKER_NAMESPACE_CONFLICT`). `undefined` while healthy. Mirrors the
   * Python (`_fatal_error`) and Rust (`fatal_error()`) SDKs.
   */
  getFatalError = (): RegistrationRejectedError | undefined => this.fatalError

  /**
   * Gracefully shutdown the iii, cleaning up all resources.
   */
  shutdown = async (): Promise<void> => {
    this.isShuttingDown = true

    this.stopMetricsReporting()

    // Shutdown OpenTelemetry
    await shutdownOtel()

    // Clear reconnection timeout and keepalive heartbeat (shutdown never
    // reaches onSocketClose: listeners are removed before close() below)
    this.clearReconnectTimeout()
    this.stopHeartbeat()

    // Reject all pending invocations
    for (const [_id, invocation] of this.invocations) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      invocation.reject(new Error('iii is shutting down'))
    }
    this.invocations.clear()

    // Close WebSocket. Swallow any close-time errors (most commonly
    // "WebSocket was closed before the connection was established",
    // emitted when `close()` fires while still in CONNECTING state
    // and there's no error listener). Without a catch-all listener,
    // that event becomes an unhandled exception because we remove
    // every listener right above the close call.
    if (this.ws) {
      this.ws.removeAllListeners()
      this.ws.on('error', () => {})
      try {
        this.ws.close()
      } catch {
        // ignore, shutting down anyway
      }
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
    this.ws = new WebSocket(this.address, {
      headers: this.options?.headers,
      handshakeTimeout: WS_HANDSHAKE_TIMEOUT_MS,
    })
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

  private startHeartbeat(): void {
    this.stopHeartbeat()
    // Dedicated deadline, refresh()ed on every inbound frame, so it fires
    // exactly WS_IDLE_TIMEOUT_MS after the last frame instead of on the next
    // ping tick (which could add up to a full WS_PING_INTERVAL_MS of delay).
    this.idleTimeout = setTimeout(() => {
      this.logError(`No inbound data for ${WS_IDLE_TIMEOUT_MS}ms, terminating connection to force reconnect`)
      this.ws?.terminate() // 'close' fires -> onSocketClose stops the heartbeat and schedules reconnect
    }, WS_IDLE_TIMEOUT_MS)
    this.heartbeatInterval = setInterval(() => {
      if (this.ws && this.isOpen()) {
        this.ws.ping()
      }
    }, WS_PING_INTERVAL_MS)
  }

  private stopHeartbeat(): void {
    if (this.heartbeatInterval) {
      clearInterval(this.heartbeatInterval)
      this.heartbeatInterval = undefined
    }
    if (this.idleTimeout) {
      clearTimeout(this.idleTimeout)
      this.idleTimeout = undefined
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
    this.stopHeartbeat()
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

    // Reset the idle deadline on any inbound frame (message/ping/pong), Rust SDK parity
    const touch = () => {
      this.idleTimeout?.refresh()
    }
    this.ws?.on('message', touch)
    this.ws?.on('ping', touch)
    this.ws?.on('pong', touch)
    this.startHeartbeat()

    // Reconnect: present the previous engine-assigned identity BEFORE the
    // metadata announce and registration replay so the engine retires the old
    // connection and the replay lands on a clean slate instead of racing its
    // cleanup. This must precede registerWorkerMetadata(): otherwise the old
    // connection still holds this (namespace, worker_name) and the announce
    // would trip WORKER_NAMESPACE_CONFLICT against ourselves. The token proves
    // we ARE that worker (ids alone are publicly listable).
    if (this.workerId) {
      this.sendMessageRaw(
        JSON.stringify({
          type: MessageType.Reattach,
          previous_worker_id: this.workerId,
          reattach_token: this.reattachToken,
        }),
      )
    }

    // Announce worker metadata (carrying the namespace) before flushing any
    // registrations so the engine knows this connection's namespace up front.
    this.registerWorkerMetadata()


    this.triggerTypes.forEach(({ message }) => {
      this.sendMessage(MessageType.RegisterTriggerType, message, true)
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

  private logWarn(message: string): void {
    const otelLogger = getLogger()
    if (otelLogger) {
      otelLogger.emit({
        severityNumber: SeverityNumber.WARN,
        body: `[iii] ${message}`,
      })
    } else {
      console.warn(`[iii] ${message}`)
    }
  }

  /**
   * Handle a `registrationrejected` from the engine. The `code` decides the
   * severity:
   *
   * - {@link WORKER_NAMESPACE_CONFLICT}: another live worker already holds this
   *   `(namespace, worker_name)`. The engine closes the connection, so this is
   *   fatal -- stop the worker, do not reconnect, and surface a
   *   {@link RegistrationRejectedError} to every pending invocation.
   * - {@link FUNCTION_NAMESPACE_CONFLICT}: another live worker in this namespace
   *   already exports this one function id. Only that registration is refused;
   *   the engine keeps the connection open and the worker keeps serving its
   *   other functions. Non-fatal -- log a warning and continue. Here
   *   `worker_name` carries the contested function id (the engine reuses the
   *   struct).
   * - Any other code: treated as fatal (safe default).
   */
  private onRegistrationRejected(init: {
    code: string
    namespace: string
    worker_name: string
    owner_worker_id: string
  }): void {
    if (init.code === FUNCTION_NAMESPACE_CONFLICT) {
      this.logWarn(
        `Function registration rejected: function "${init.worker_name}" in namespace "${init.namespace}" is already exported by worker ${init.owner_worker_id}. The worker stays connected and keeps serving its other functions.`,
      )
      return
    }

    const error = new RegistrationRejectedError(init)
    // Record it so callers can poll for the terminal cause (parity with the
    // Python/Rust SDKs); the connection also transitions to `failed` below.
    this.fatalError = error
    if (init.code === WORKER_NAMESPACE_CONFLICT) {
      this.logError('Registration rejected by engine', error)
    } else {
      this.logError(`Registration rejected by engine with unknown code "${init.code}"; treating as fatal`, error)
    }

    // Mark shutdown first so neither onSocketClose nor scheduleReconnect can
    // schedule a reconnect for a rejection that would only be rejected again.
    this.isShuttingDown = true
    this.clearReconnectTimeout()
    this.stopHeartbeat()

    // Reject every in-flight invocation with the fatal error.
    for (const [, invocation] of this.invocations) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      invocation.reject(error)
    }
    this.invocations.clear()

    // Tear down the socket without triggering the reconnect path.
    if (this.ws) {
      this.ws.removeAllListeners()
      this.ws.on('error', () => {})
      try {
        this.ws.terminate()
      } catch {
        // ignore, stopping anyway
      }
      this.ws = undefined
    }

    this.stopMetricsReporting()
    this.setConnectionState('failed')
  }

  private onInvocationResult(invocation_id: string, result: unknown, error: unknown): void {
    const invocation = this.invocations.get(invocation_id)

    if (invocation) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      if (error) {
        invocation.reject(this.toInvocationError(error, invocation.function_id))
      } else {
        invocation.resolve(result)
      }
    }

    this.invocations.delete(invocation_id)
  }

  /**
   * Wrap a wire-format `ErrorBody` in {@link InvocationError} so callers get
   * a real `Error` with a readable `.message` and a typed `.code`. Pass-through
   * for values that are already `Error` subclasses. Everything else is wrapped
   * under an `UNKNOWN` code so `String(err) !== '[object Object]'` holds for
   * every rejection path.
   */
  private toInvocationError(error: unknown, function_id?: string): Error {
    if (error instanceof Error) {
      return error
    }
    if (isErrorBody(error)) {
      return new InvocationError({
        code: error.code,
        message: error.message,
        function_id,
        stacktrace: error.stacktrace,
      })
    }
    // JSON.stringify(undefined) returns undefined (not "undefined"), which
    // would set message to the literal string "undefined" after type coercion
    // and leak an uninformative rejection. Fall back through String(error)
    // so every path produces a concrete, readable string.
    const message =
      typeof error === 'string'
        ? error
        : (JSON.stringify(error) ?? String(error))
    return new InvocationError({
      code: 'UNKNOWN',
      message,
      function_id,
    })
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
    metadata?: JsonValue,
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
          await fn.handler(resolvedInput, metadata, traceparent, baggage)
        } catch (error) {
          this.logError(`Error invoking function ${function_id}`, error)
        }
        return
      }

      try {
        const result = await fn.handler(resolvedInput, metadata, traceparent, baggage)
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

  private async onRegisterTrigger(message: {
    trigger_type: string
    id: string
    function_id: string
    config: unknown
    metadata?: Record<string, unknown>
    namespace?: string
  }) {
    const { trigger_type, id, function_id, config, metadata, namespace } = message
    const triggerTypeData = this.triggerTypes.get(trigger_type)

    if (triggerTypeData) {
      try {
        // Surface the namespace to the handler: a provider that fires the target
        // later needs it to resolve in the right namespace, not `default`.
        await triggerTypeData.handler.registerTrigger({ id, function_id, config, metadata, namespace })
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

  private async onUnregisterTrigger(message: {
    trigger_type?: string
    id: string
    function_id?: string
    config?: unknown
    metadata?: Record<string, unknown>
  }) {
    const trigger_type = message.trigger_type
    if (!trigger_type) return

    const triggerTypeData = this.triggerTypes.get(trigger_type)
    if (!triggerTypeData) return

    const { id, function_id = '', config, metadata } = message
    try {
      await triggerTypeData.handler.unregisterTrigger({ id, function_id, config, metadata })
    } catch (error) {
      this.logError(`Error unregistering trigger ${id}`, error)
    }
  }

  private onTriggerRegistrationResult(
    message: { id: string; trigger_type?: string; type?: string; function_id: string; error?: { code: string; message: string; stacktrace?: string } },
  ): void {
    if (!message.error) return
    const triggerType = message.trigger_type ?? message.type ?? ''
    console.error(
      `[iii] Trigger registration failed for "${message.id}" (${triggerType}): ${message.error.message}`,
    )
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
      const { invocation_id, function_id, data, metadata, traceparent, baggage } = message as InvokeFunctionMessage
      this.onInvokeFunction(invocation_id, function_id, data, metadata, traceparent, baggage)
    } else if (msgType === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(message as { trigger_type: string; id: string; function_id: string; config: unknown; metadata?: Record<string, unknown> })
    } else if (msgType === MessageType.UnregisterTrigger) {
      this.onUnregisterTrigger(
        message as {
          trigger_type?: string
          id: string
          function_id?: string
          config?: unknown
          metadata?: Record<string, unknown>
        },
      )
    } else if (msgType === MessageType.TriggerRegistrationResult) {
      this.onTriggerRegistrationResult(
        message as { id: string; trigger_type?: string; type?: string; function_id: string; error?: { code: string; message: string; stacktrace?: string } },
      )
    } else if (msgType === MessageType.WorkerRegistered) {
      const { worker_id, reattach_token } = message as WorkerRegisteredMessage
      this.workerId = worker_id
      this.reattachToken = reattach_token
      console.debug('[iii] Worker registered with ID:', worker_id)
      this.startMetricsReporting()
    } else if (msgType === MessageType.RegistrationRejected) {
      const { code, namespace, worker_name, owner_worker_id } = message as RegistrationRejectedMessage
      this.onRegistrationRejected({ code, namespace, worker_name, owner_worker_id })
    }
  }
}

/**
 * Factory object that constructs routing actions for {@link IIIClient.trigger}.
 *
 * @example
 * ```typescript
 * import { TriggerAction } from 'iii-sdk'
 *
 * // Enqueue to a named queue
 * worker.trigger({
 *   function_id: 'process',
 *   payload: { data: 'hello' },
 *   action: TriggerAction.Enqueue({ queue: 'jobs' }),
 * })
 *
 * // Fire-and-forget
 * worker.trigger({
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
   * Requires a queue worker in the project. Run `iii worker add queue`.
   * Without it the trigger rejects with `enqueue_error` (no queue provider).
   *
   * @param opts - Queue routing options.
   * @param opts.queue - Name of the target queue.
   */
  Enqueue: (opts: { queue: string }) => ({ type: 'enqueue' as const, ...opts }),
  /**
   * Fire-and-forget routing. The engine forwards the invocation without
   * waiting for a response or queuing the job.
   */
  Void: () => ({ type: 'void' as const }),
} as const

/**
 * Register the worker with a iii instance, returns a connected worker client.
 * The WebSocket connection is established automatically.
 *
 * @param address - WebSocket URL of the III engine (e.g. `ws://localhost:49134`).
 * @param options - Optional {@link InitOptions} for worker name, timeouts, reconnection, and OTel.
 * @returns A connected {@link IIIClient} instance.
 *
 * @example
 * ```typescript
 * import { registerWorker } from 'iii-sdk'
 *
 * const worker = registerWorker(process.env.III_URL ?? 'ws://localhost:49134', {
 *   workerName: 'my-worker',
 * })
 * ```
 */
export const registerWorker = (address: string, options?: InitOptions): IIIClient => new Sdk(address, options)
