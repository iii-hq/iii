import { createRequire } from 'node:module'
import * as os from 'node:os'
import { context, trace } from '@opentelemetry/api'
import { type Data, WebSocket } from 'ws'
import { ChannelReader, ChannelWriter } from './channels'
import { InvocationError, isErrorBody, RegistrationRejectedError } from './errors'
import { compileFilter, FilterError } from './filter'
import { announceProcess } from './process-marker'
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

// Main JSON outbound single-frame threshold: 16 MiB (16,777,216 bytes).
const MAX_JSON_FRAME_BYTES = 16 * 1024 * 1024
// Payload size for each WebSocket fragment when an envelope exceeds the single-frame threshold.
const MAX_JSON_FRAGMENT_BYTES = 1024 * 1024
// Main JSON message limit, outbound and inbound: 64 MiB (67,108,864 bytes).
const MAX_JSON_MESSAGE_BYTES = 64 * 1024 * 1024

import type { HttpInvocationConfig } from '@iii-dev/helpers/http'
import {
  extractContext,
  getLogger,
  initOtel,
  injectBaggage,
  injectTraceparent,
  type OtelConfig,
  recordSpanEvent,
  redactAndTruncate,
  registerWorkerGauges,
  resolveMaxBytesFromEnv,
  SeverityNumber,
  shutdownOtel,
  stopWorkerGauges,
  withSpan,
} from '@iii-dev/helpers/observability'
import { getMeter, getTracer } from '@iii-dev/helpers/observability/internal'
import { SpanKind } from '@opentelemetry/api'
import {
  type ActivationWire,
  type CallerWire,
  type ErrorBody,
  FEATURE_OBSERVE_V1,
  type IIIMessage,
  type InvocationResultMessage,
  type JsonValue,
  MessageType,
  type RegisterFunctionMessage,
  type RegisterTriggerMessage,
  type RegisterTriggerTypeMessage,
  type RegistrationRejectedMessage,
  SDK_FEATURES,
  type StreamChannelRef,
  type TriggerRegistrationResultMessage,
  type TriggerRequest,
} from './iii-types'
import {
  type ActiveInvocation,
  currentContext,
  MAX_LOCAL_DEPTH,
  publicContext,
  runInContext,
} from './invocation-context'
import { FunctionObservers, OBSERVE_HOOK_TIMEOUT_MS, type ObserveHost } from './observable'
import type {
  InvocationContext,
  ObserveHooks,
  ObserverHandle,
  ObserverOptions,
  ObserveSource,
  Target,
} from './observe-types'
import {
  buildInvokeFunction,
  buildObserveMessage,
  buildObserverAck,
  callerFor,
  decodeFunctionRegistered,
  decodeInvokeFunction,
  decodeObserveResult,
  decodeObserverAdded,
  decodeObserverRemoved,
  decodeObserverStateChanged,
  decodeWorkerRegistered,
  observeDescriptor,
  stripCaller,
} from './observe-wire'
import { ObserverHandleImpl, toObserverState } from './observer-handle'
import { Reporter, type ReporterLimits } from './reporter'
import type { IStream } from './stream'
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
import { detectProjectName, isChannelRef, resolveHeaders } from './utils'

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

function resolveWorkerName(explicitName?: string): string {
  // III_WORKER_NAME carries the orchestrator-assigned name (set by the supervisor
  // for Compose-managed workers). The engine matches live registrations by
  // name, so that identity must win over a name embedded in worker code.
  const managedName = process.env.III_WORKER_NAME
  if (managedName) {
    return managedName
  }
  return explicitName ?? `${os.hostname()}:${process.pid}`
}

/**
 * Engine address used when neither an explicit address nor `III_URL` is set.
 * The IPv4 loopback is spelled out on purpose: `localhost` can resolve to `::1`
 * on a host whose engine only listens on IPv4.
 */
export const DEFAULT_ENGINE_URL = 'ws://127.0.0.1:49134'

function resolveAddress(address?: string): string {
  // The supervisor that spawned this process -- `iii compose`, a container
  // runtime, systemd -- sets III_URL, the same way it sets III_NAMESPACE and
  // III_WORKER_NAME. An explicit address still wins.
  if (address) {
    return address
  }
  const fromEnv = process.env.III_URL
  if (fromEnv) {
    return fromEnv
  }
  return DEFAULT_ENGINE_URL
}

/**
 * The namespace one call or one binding resolves in.
 *
 * `undefined` inherits the worker's. A blank one is refused: named and left
 * empty asks for the opposite of what absent asks for, and the two are only
 * ever confused by accident -- `??` forwards the empty string, Python's `or`
 * coerced it and Go dropped it, so one mistake produced three behaviours.
 */
function callNamespace(explicit: string | undefined, worker: string | undefined, source: string): string | undefined {
  if (explicit !== undefined && explicit.trim() === '') {
    throw new Error(
      `namespace is empty: ${source} was set to ${JSON.stringify(explicit)}. ` +
        "Give it a name, or leave it unset to stay in this worker's namespace.",
    )
  }
  return explicit ?? worker
}

/** Resolve one function invocation without leaking engine and configuration builtins into a worker namespace. */
function invocationNamespace(
  explicit: string | undefined,
  worker: string | undefined,
  functionId: string,
  source = 'TriggerRequest.namespace',
): string | undefined {
  const namespace = callNamespace(explicit, worker, source)
  return explicit === undefined && (functionId.startsWith('engine::') || functionId.startsWith('configuration::'))
    ? 'default'
    : namespace
}

const MIGRATION_GUIDE = 'https://iii.dev/docs/upgrading/from-0-24-x'

/** CONTRACT §5: one warning per process per deprecated API. */
const DEPRECATIONS = {
  registerTrigger: `registerTrigger is deprecated and will be removed in a future release. Use worker.observe(source, target) instead. Migration guide: ${MIGRATION_GUIDE}`,
  registerTriggerType: `registerTriggerType is deprecated and will be removed in a future release. Register a function with an observe descriptor (registerFunction(id, handler, { observe: { ... } })) instead. Migration guide: ${MIGRATION_GUIDE}`,
} as const

const deprecationsWarned = new Set<keyof typeof DEPRECATIONS>()

/**
 * Local dispatch hands the handler a copy of the caller's payload and the
 * caller a copy of the result (value semantics, as over the wire):
 * `structuredClone`, or JSON for what it cannot clone (functions, class
 * instances with methods the clone would drop).
 */
function cloneValue<T>(value: T): T {
  if (value === null || typeof value !== 'object') {
    return (typeof value === 'function' || typeof value === 'symbol' ? undefined : value) as T
  }
  try {
    return structuredClone(value)
  } catch {
    const text = JSON.stringify(value)
    return (text === undefined ? undefined : JSON.parse(text)) as T
  }
}

/** Frames only an engine with `observe.v1` understands (CONTRACT D2). */
const OBSERVE_FAMILY: ReadonlySet<MessageType> = new Set([
  MessageType.Observe,
  MessageType.Unobserve,
  MessageType.ObserverAck,
  MessageType.InvocationsObserved,
  MessageType.LocalInvocationStats,
])

const UNSUPPORTED_REASON = {
  code: 'UNSUPPORTED',
  message: 'the engine does not support observe (no observe.v1 in its features); upgrade it to 0.25 or later',
}

/** `args_format` needs `delivery: 'owner'`: an auto function's events are its invocations, offered to every observer. */
function validateObserveHooks(functionId: string, hooks: ObserveHooks | undefined): void {
  if (!hooks) return
  const delivery = hooks.delivery
  if (delivery !== undefined && delivery !== 'auto' && delivery !== 'owner') {
    throw new InvocationError({
      code: 'INVALID_OBSERVE_DESCRIPTOR',
      message: `observe.delivery must be 'auto' or 'owner', got ${JSON.stringify(delivery)}`,
      function_id: functionId,
    })
  }
  if (hooks.args_format !== undefined && delivery !== 'owner') {
    throw new InvocationError({
      code: 'INVALID_OBSERVE_DESCRIPTOR',
      message: "observe.args_format requires delivery: 'owner' (an auto function takes no args)",
      function_id: functionId,
    })
  }
}

/** The error a handler's failure becomes, the same on the local and the remote path. */
function handlerErrorBody(error: unknown): ErrorBody {
  const isError = error instanceof Error
  return {
    code: 'invocation_failed',
    message: isError ? error.message : String(error),
    stacktrace: isError ? error.stack : undefined,
  }
}

function resolveNamespace(optionNamespace?: string): string | undefined {
  // III_NAMESPACE carries the namespace for managed workers (set by iii-worker
  // at spawn), mirroring III_WORKER_NAME. An explicit option wins; otherwise
  // the env var provides the managed identity. Absent in both means undefined
  // -- the engine applies its `default` namespace when none is on the wire.
  //
  // A declared-but-blank namespace throws rather than reading as "no
  // namespace". The two mean opposite things: absent asks for the engine's
  // default, blank names a namespace and gives nothing to name it with. Read
  // as absent, the worker registers in `default`, and every call and trigger it
  // makes now follows it there -- a whole project quietly serving from the
  // wrong namespace, and the one thing an operator cannot see by reading the
  // declaration.
  if (optionNamespace !== undefined) {
    if (optionNamespace.trim() === '') {
      throw new Error(
        `namespace is empty: options.namespace was set to ${JSON.stringify(optionNamespace)}. ` +
          'Give it a name, or leave it unset to register in `default`.',
      )
    }
    return optionNamespace
  }
  // III_NAMESPACE is left alone when blank. `FOO=` is how a shell says "not
  // set" -- `III_NAMESPACE=${NS}` with NS unset produces exactly that -- so
  // reading it as absent is what the caller meant, and absent is a namespace a
  // worker may legitimately have none of. Only the option is a mistake: nobody
  // writes a namespace parameter and passes nothing on purpose.
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
  /** Display name for this worker. A non-empty `III_WORKER_NAME` overrides it. Defaults to `hostname:pid`. */
  workerName?: string
  /**
   * Namespace this worker belongs to. Resolution order:
   * `options.namespace` -> `process.env.III_NAMESPACE` -> undefined. When
   * undefined the engine applies its `default` namespace.
   *
   * It scopes more than the registration. The worker and its functions
   * register here, so identically-named entries coexist across namespaces, and
   * everything the worker does afterwards follows it: a `trigger` resolves its
   * target here, and a `registerTrigger` binds here, unless the call names
   * another namespace. One declaration places the whole worker.
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
   * The `engineWsUrl` is set automatically from the III address, and `engineWsHeaders`
   * from the handshake headers of the main connection.
   */
  otel?: Omit<OtelConfig, 'engineWsUrl' | 'engineWsHeaders'>
  /**
   * Custom HTTP headers sent during the WebSocket handshake, on the main connection and
   * on streaming channels. `Authorization: Bearer $III_TOKEN` is added unless you set
   * `Authorization` here.
   */
  headers?: Record<string, string>
  /** @internal */
  telemetry?: TelemetryOptions
  /** @internal Test knobs for the observe hooks and the reporter. */
  observe?: { hookTimeoutMs?: number; reporter?: Partial<ReporterLimits> }
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
  // Observers this worker created, replayed (same id) on every connection.
  private observers = new Map<string, ObserverHandleImpl>()
  // The engine's features (`WorkerRegistered.features`), kept across a
  // disconnect until the next `WorkerRegistered` replaces them.
  private engineFeatures: ReadonlySet<string> = new Set()
  // `WorkerRegistered` arrived on the current connection.
  private registeredOnConnection = false
  // Bumped on every connection: `functionregistered` confirms a function on one.
  private connectionEpoch = 0
  // Observe-family frames that wait for the next connection's features.
  private observeQueue: Record<string, unknown>[] = []
  private hooksInactiveWarned = new Set<string>()
  private readonly reporter: Reporter
  private readonly observeHost: ObserveHost
  private removeProcessMarker: () => void

  constructor(
    private readonly address: string,
    private readonly options?: InitOptions,
  ) {
    this.workerName = resolveWorkerName(options?.workerName)
    this.namespace = resolveNamespace(options?.namespace)
    this.workerDescription = options?.workerDescription
    this.metricsReportingEnabled = options?.enableMetricsReporting ?? true
    this.invocationTimeoutMs = options?.invocationTimeoutMs ?? DEFAULT_INVOCATION_TIMEOUT_MS
    this.reconnectionConfig = {
      ...DEFAULT_BRIDGE_RECONNECTION_CONFIG,
      ...options?.reconnectionConfig,
    }

    this.reporter = new Reporter(
      {
        online: () => this.observeOnline(),
        status: (functionId) => {
          const state = this.functions.get(functionId)?.observe
          if (!state || state.releasedEpoch === this.connectionEpoch) return 'gone'
          return state.confirmedEpoch === this.connectionEpoch ? 'ready' : 'wait'
        },
        sendFrame: (data) => {
          if (!this.isOpen()) return false
          this.sendMessageRaw(data)
          return true
        },
        pendingBytes: () => this.ws?.bufferedAmount ?? 0,
        logError: (message, error) => this.logError(message, error),
      },
      options?.observe?.reporter,
    )
    this.observeHost = {
      identity: () =>
        this.workerId !== undefined
          ? { worker_id: this.workerId, worker_name: this.workerName }
          : { worker_name: this.workerName },
      sendAck: (ack) => this.sendMessage(MessageType.ObserverAck, buildObserverAck(ack), true),
      reporter: this.reporter,
      callTarget: (target, observerId, payload, timeoutMs) =>
        this.invokeRemote({ function_id: target, payload, timeoutMs }, undefined, { observer_id: observerId }),
      logError: (message, error) => this.logError(message, error),
      hookTimeoutMs: () => options?.observe?.hookTimeoutMs ?? OBSERVE_HOOK_TIMEOUT_MS,
      traceContext: () => ({ traceparent: injectTraceparent(), baggage: injectBaggage() }),
    }

    // Initialize OpenTelemetry (enabled by default, opt-out via config or env)
    initOtel({
      ...options?.otel,
      engineWsUrl: this.address,
      engineWsHeaders: resolveHeaders(options?.headers),
    })

    // Lets `iii ps` find this worker however it was started.
    this.removeProcessMarker = announceProcess(this.address, this.workerName)

    this.reporter.start()
    this.connect()
  }

  private warnDeprecated(api: keyof typeof DEPRECATIONS): void {
    if (deprecationsWarned.has(api)) return
    deprecationsWarned.add(api)
    this.logWarn(DEPRECATIONS[api])
  }

  /**
   * Registers a custom trigger type with the engine. A trigger type defines
   * how external events (HTTP, cron, queue, etc.) map to function invocations.
   *
   * @deprecated Register a function with an observe descriptor instead:
   * `registerFunction(id, handler, { observe: { delivery: 'owner', onObserverAdded, onObserverRemoved } })`.
   * Logs a warning once per process. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
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
    this.warnDeprecated('registerTriggerType')
    const message = this.snapshotRegistration<RegisterTriggerTypeMessage>({
      ...triggerType,
      message_type: MessageType.RegisterTriggerType,
    })
    this.sendMessage(MessageType.RegisterTriggerType, message, true)
    this.triggerTypes.set(message.id, {
      message,
      handler,
    })

    return {
      id: message.id,
      // This typed helper pairs a function with its trigger, so it defaults the
      // trigger's namespace to this worker's — otherwise the function would land
      // in the worker's namespace and the trigger in `default`, and never resolve
      // it. The low-level `registerTrigger` keeps the engine default (`default`).
      registerTrigger: (functionId: string, config: TConfig, metadata?: Record<string, unknown>) => {
        return this.registerTrigger({
          type: message.id,
          function_id: functionId,
          config,
          metadata,
          namespace: this.namespace,
        })
      },
      registerFunction: (functionId, handler, config, metadata?) => {
        const ref = this.registerFunction(functionId, handler)
        this.registerTrigger({
          type: message.id,
          function_id: functionId,
          config,
          metadata,
          namespace: this.namespace,
        })
        return ref
      },
      unregister: () => {
        this.unregisterTriggerType(message)
      },
    }
  }

  /**
   * Unregisters a previously registered trigger type.
   *
   * @deprecated Trigger types are replaced by functions with an observe
   * descriptor; unregister the function instead. Migration guide:
   * https://iii.dev/docs/upgrading/from-0-24-x
   *
   * @param triggerType - The trigger type to unregister (must match the `id` used during registration).
   */
  unregisterTriggerType = (triggerType: Omit<RegisterTriggerTypeMessage, 'message_type'>): void => {
    // The engine reads `{ id, namespace? }` (sdk/fixtures/protocol/observe/unregistertriggertype.json).
    const { id, namespace } = triggerType
    this.sendMessage(MessageType.UnregisterTriggerType, namespace !== undefined ? { id, namespace } : { id }, true)
    this.triggerTypes.delete(id)
  }

  /**
   * Binds a trigger configuration to a registered function. When the trigger
   * fires, the engine invokes the target function.
   *
   * @deprecated Use `iii.observe(source, target)` instead. Logs a warning once
   * per process. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
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
    this.warnDeprecated('registerTrigger')
    const id = crypto.randomUUID()
    const fullTrigger: RegisterTriggerMessage = this.snapshotRegistration({
      ...trigger,
      id,
      message_type: MessageType.RegisterTrigger,
      // Unset means this worker's namespace, not the engine's default. A
      // trigger names a function, and the function a worker registers lands in
      // the worker's namespace, so defaulting anywhere else registers a trigger
      // that fires and resolves nothing. Naming another namespace, `default`
      // included, stays a matter of saying so.
      ...(() => {
        const namespace = callNamespace(trigger.namespace, this.namespace, 'RegisterTriggerInput.namespace')
        return namespace !== undefined ? { namespace } : {}
      })(),
    })
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

    // The hooks stay here; only the descriptor's four fields travel, and
    // `localDispatch` is this SDK's alone.
    const { observe: hooks, localDispatch, ...wireOptions } = options ?? {}
    validateObserveHooks(functionId, hooks)
    const descriptor = observeDescriptor(hooks)
    const registration = descriptor ? { ...wireOptions, observe: descriptor } : wireOptions

    const isHandler = typeof handlerOrInvocation === 'function'

    const fullMessage: RegisterFunctionMessage = this.snapshotRegistration(
      isHandler
        ? { ...registration, id: functionId, message_type: MessageType.RegisterFunction }
        : {
            ...registration,
            id: functionId,
            message_type: MessageType.RegisterFunction,
            invocation: {
              url: handlerOrInvocation.url,
              method: handlerOrInvocation.method ?? 'POST',
              timeout_ms: handlerOrInvocation.timeout_ms,
              headers: handlerOrInvocation.headers,
              auth: handlerOrInvocation.auth,
            },
          },
    )

    this.sendMessage(MessageType.RegisterFunction, fullMessage, true)

    const observe = new FunctionObservers(functionId, hooks, localDispatch !== false, isHandler, this.observeHost)
    if (observe.needsObserve && this.registeredOnConnection && !this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      this.warnHooksInactive(functionId)
    }

    if (isHandler) {
      const handler = handlerOrInvocation as RemoteFunctionHandler
      this.functions.set(functionId, {
        message: fullMessage,
        observe,
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
                const { json, truncated } = redactAndTruncate({ error: errMsg }, payloadMaxBytes)
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
      this.functions.set(functionId, { message: fullMessage, observe })
    }

    return {
      id: functionId,
      unregister: () => {
        // A stale ref (the id was unregistered and registered again) must not
        // touch the registration that replaced it: nothing to do.
        if (this.functions.get(functionId)?.observe !== observe) return
        this.sendMessage(MessageType.UnregisterFunction, { id: functionId }, true)
        this.functions.delete(functionId)
        observe.dispose()
      },
      isObserved: () => observe.isObserved(),
      observers: () => observe.observers(),
    }
  }

  /**
   * Observes a function: every event of `source` (an invocation of an auto
   * function, or what its owner hands over) that passes `source.filters`
   * invokes `target`, a registered function.
   *
   * Returns synchronously, also before the connection is up; the observer is
   * replayed with the same id on every reconnect. Errors (`INVALID_ARGS`,
   * `INVALID_FILTER`, `FORBIDDEN`, an owner's rejection, `UNSUPPORTED` against
   * an engine without observe) show up as state `error` and reject `ready()`.
   *
   * @param source - What is observed: `function_id`, `args` (owner-delivered
   *   functions only), `filters`, and `on` / `capture` / `deliver`.
   * @param target - The function to invoke (registered first; until then the observer is `pending`).
   * @param options - Policy and metadata: `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * iii.registerFunction('alerts::payment-failed', async ({ input, error }: Observed) => notify(input, error))
   *
   * const failures = iii.observe(
   *   { function_id: 'payments::charge', on: 'error', filters: { 'input.amount': { $gte: 500 } } },
   *   'alerts::payment-failed',
   * )
   * await failures.ready()
   * // later
   * await failures.dispose()
   * ```
   */
  observe = (source: ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle => {
    if (!source || typeof source.function_id !== 'string' || source.function_id.trim() === '') {
      throw new Error('observe: source.function_id is required')
    }
    const targetId = typeof target === 'string' ? target : target?.function_id
    if (typeof targetId !== 'string' || targetId.trim() === '') {
      throw new Error('observe: target is required (the id of a registered function)')
    }
    const id = options?.id ?? crypto.randomUUID()
    if (this.observers.has(id)) {
      throw new Error(`observe: observer id already in use: ${id}`)
    }
    const onDispose = (h: ObserverHandleImpl) => this.disposeObserver(h)
    const logError = (msg: string, error?: unknown) => this.logError(msg, error)
    // Nothing is sent for an observer that cannot be valid; its handle says why.
    const refused = (reason: { code: string; message: string }) => {
      const handle = new ObserverHandleImpl(
        id,
        { id, source: { function_id: source.function_id }, target: { function_id: targetId } },
        onDispose,
        logError,
      )
      handle.setState('error', reason)
      return handle
    }
    // Validated as written, before JSON could drop or rewrite a clause (an
    // `undefined` operand, `NaN`, a `Date`, a `Map`) and widen the filter.
    if (source.filters !== undefined) {
      try {
        compileFilter(source.filters)
      } catch (error) {
        if (!(error instanceof FilterError)) throw error
        return refused({ code: error.code, message: error.message })
      }
    }
    let message: ReturnType<typeof buildObserveMessage>
    try {
      message = buildObserveMessage(id, source, target, options, (explicit, functionId, field) =>
        invocationNamespace(explicit, this.namespace, functionId, field),
      )
    } catch (error) {
      if (!(error instanceof TypeError)) throw error
      // Not JSON (a BigInt, a cycle in the args or options)
      return refused({ code: 'INVALID_ARGS', message: `observe: the source or options are not JSON: ${error.message}` })
    }
    const handle = new ObserverHandleImpl(id, message, onDispose, logError)
    this.observers.set(id, handle)
    if (this.registeredOnConnection && this.isOpen()) {
      if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) this.sendObserve(handle)
      else handle.setState('error', UNSUPPORTED_REASON)
    }
    return handle
  }

  /**
   * The invocation the calling code runs in (in every handler, remote or
   * local), or `undefined` outside any handler.
   *
   * @example
   * ```typescript
   * iii.registerFunction('orders::create', async (order) => {
   *   const inv = iii.currentInvocation() // { function_id, invocation_id, caller, observe_depth }
   *   return { id: inv?.invocation_id }
   * })
   * ```
   */
  currentInvocation = (): InvocationContext | undefined => publicContext(currentContext())

  private sendObserve(handle: ObserverHandleImpl): void {
    this.sendMessage(MessageType.Observe, handle.message, true)
    handle.sent = true
  }

  private disposeObserver(handle: ObserverHandleImpl): void {
    if (this.observers.get(handle.id) === handle) this.observers.delete(handle.id)
    if (!handle.sent) return
    this.sendObserveFamily(MessageType.Unobserve, { id: handle.id })
  }

  /**
   * An observe-family frame that must not be lost to a disconnect (`unobserve`:
   * a reattach would otherwise keep the observer alive). Without a registered
   * connection it waits for the next `WorkerRegistered`, and goes out only if
   * that engine advertises `observe.v1`.
   */
  private sendObserveFamily(type: MessageType, message: Record<string, unknown>): void {
    if (!this.isOpen() || !this.registeredOnConnection) {
      this.observeQueue.push({ type, ...message })
    } else if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      this.sendMessage(type, message, true)
    }
  }

  private warnHooksInactive(functionId: string): void {
    if (this.hooksInactiveWarned.has(functionId)) return
    this.hooksInactiveWarned.add(functionId)
    this.logWarn(
      `Function "${functionId}" declares observe hooks or owner delivery, but the engine does not support observe (observe.v1): it is registered as a plain function and its observe hooks are inactive.`,
    )
  }

  /** Connected to an engine that advertised `observe.v1`. */
  private observeOnline(): boolean {
    return this.isOpen() && this.registeredOnConnection && this.engineFeatures.has(FEATURE_OBSERVE_V1)
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
      writer: new ChannelWriter(this.address, result.writer, this.options?.headers),
      reader: new ChannelReader(this.address, result.reader, this.options?.headers),
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
  trigger = async <TInput = unknown, TOutput = any>(request: TriggerRequest<TInput>): Promise<TOutput> => {
    const { function_id, action } = request
    // Engine-owned builtins stay in `default`. Other implicit calls stay in
    // this worker's namespace. An explicit request namespace always wins.
    const namespace = invocationNamespace(request.namespace, this.namespace, function_id)

    // Local dispatch (sdk.md §3): a function this worker hosts, confirmed by
    // the engine for in-process calls, in the namespace it registered in.
    // `enqueue` expects a queue receipt, so it always goes to the engine, and
    // so does a call already MAX_LOCAL_DEPTH local calls deep.
    if (request.dispatch !== 'remote' && action?.type !== 'enqueue') {
      const fn = this.functions.get(function_id)
      const observe = fn?.observe
      if (
        fn?.handler &&
        observe?.localDispatch &&
        (namespace === undefined || namespace === observe.namespace) &&
        (currentContext()?.localDepth ?? 0) < MAX_LOCAL_DEPTH
      ) {
        return this.invokeLocal<TOutput>(function_id, fn.handler, observe, request)
      }
    }
    return this.invokeRemote<TOutput>(request, namespace)
  }

  /** `caller` and `observe_depth` an outgoing call carries from the current handler (engines with `observe.v1`). */
  private outgoingObserveFields(): { caller?: CallerWire; observe_depth?: number } {
    const cur = currentContext()
    if (!cur || !this.engineFeatures.has(FEATURE_OBSERVE_V1)) return {}
    return { caller: callerFor(cur), observe_depth: cur.observe_depth }
  }

  /** The routed path: an `InvokeFunction` through the engine. */
  private invokeRemote<TOutput>(
    request: TriggerRequest<unknown>,
    namespace: string | undefined,
    activation?: ActivationWire,
  ): Promise<TOutput> {
    const { function_id, payload, action, timeoutMs, metadata } = request
    const effectiveTimeout = timeoutMs ?? this.invocationTimeoutMs
    const observeFields = this.outgoingObserveFields()

    // Void is fire-and-forget, no invocation_id, no response
    if (action?.type === 'void') {
      const traceparent = injectTraceparent()
      const baggage = injectBaggage()
      this.sendMessage(
        MessageType.InvokeFunction,
        buildInvokeFunction({
          function_id,
          data: payload,
          traceparent,
          baggage,
          action,
          metadata,
          // Omitted only when this worker has no namespace either.
          namespace,
          ...observeFields,
          activation,
        }) as Omit<IIIMessage, 'message_type'>,
      )
      return Promise.resolve(undefined as TOutput)
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

      this.sendMessage(
        MessageType.InvokeFunction,
        buildInvokeFunction({
          invocation_id,
          function_id,
          data: payload,
          traceparent,
          baggage,
          action,
          metadata,
          // Omitted only when this worker has no namespace either.
          namespace,
          ...observeFields,
          activation,
        }) as Omit<IIIMessage, 'message_type'>,
      )
    })
  }

  /**
   * The local path (sdk.md §4.3): runs the handler in-process, with a copy of
   * the payload, inside a new invocation context; the same timeout and error
   * shape as the remote path; reports the call when an observer matches.
   * Puts nothing on the wire when nobody observes the function.
   */
  private invokeLocal<TOutput>(
    function_id: string,
    handler: NonNullable<RemoteFunctionData['handler']>,
    observe: FunctionObservers,
    request: TriggerRequest<unknown>,
  ): Promise<TOutput> {
    const parent = currentContext()
    const caller: CallerWire =
      this.workerId !== undefined
        ? { worker_id: this.workerId, worker_name: this.workerName }
        : { worker_name: this.workerName }
    if (parent) {
      caller.function_id = parent.function_id
      caller.invocation_id = parent.invocation_id
      const grandparent = stripCaller(parent.caller)
      if (grandparent && Object.keys(grandparent).length > 0) caller.parent = grandparent
    }
    const traceparent = injectTraceparent()
    const baggage = injectBaggage()
    const ctx: ActiveInvocation = {
      function_id,
      invocation_id: crypto.randomUUID(),
      caller,
      observe_depth: parent?.observe_depth ?? 0,
      metadata: request.metadata,
      traceparent,
      baggage,
      localDepth: (parent?.localDepth ?? 0) + 1,
    }

    // The observers this call is evaluated against: those active when it
    // started. None → it is not reported, and its input is not copied for it.
    const observed = observe.delivery === 'auto' ? observe.compiled : []
    let input: unknown
    let observedInput: unknown
    let observedMetadata: unknown
    let metadata: unknown
    try {
      // What observers see is the payload and metadata as the caller passed them
      // when the call started, not what the handler or the caller made of them since.
      if (observed.length > 0) {
        observedInput = cloneValue(request.payload)
        observedMetadata = request.metadata === undefined ? undefined : cloneValue(request.metadata)
      }
      input = this.resolveChannelValue(cloneValue(request.payload))
      metadata = request.metadata === undefined ? undefined : cloneValue(request.metadata)
    } catch (error) {
      return Promise.reject(error)
    }

    const startedAt = Date.now()
    const t0 = performance.now()
    const finish = (ok: boolean, output: unknown, error: ErrorBody | undefined) => {
      try {
        const duration = performance.now() - t0
        this.reporter.recordLocal(function_id, ok, duration)
        observe.reportLocal(observed, {
          invocation_id: ctx.invocation_id,
          ok,
          input: observedInput,
          output,
          error,
          caller: ctx.caller,
          metadata: observedMetadata,
          started_at_ms: startedAt,
          duration_ms: duration,
          observe_depth: ctx.observe_depth,
          traceparent,
          baggage,
        })
      } catch (reportError) {
        this.logError(`Failed to report a local invocation of ${function_id}`, reportError)
      }
    }

    const done = runInContext(ctx, () => handler(input, metadata as JsonValue, traceparent, baggage)).then(
      (output) => {
        finish(true, output, undefined)
        return output
      },
      (error) => {
        const body = handlerErrorBody(error)
        finish(false, undefined, body)
        throw new InvocationError({ ...body, function_id })
      },
    )

    if (request.action?.type === 'void') {
      done.catch((error) => this.logError(`Error invoking function ${function_id}`, error))
      return Promise.resolve(undefined as TOutput)
    }

    const effectiveTimeout = request.timeoutMs ?? this.invocationTimeoutMs
    return new Promise<TOutput>((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(
          new InvocationError({
            code: 'TIMEOUT',
            message: `invocation timed out after ${effectiveTimeout}ms`,
            function_id,
          }),
        )
      }, effectiveTimeout)
      done.then(
        (output) => {
          clearTimeout(timeout)
          try {
            resolve(cloneValue(output) as TOutput)
          } catch (error) {
            reject(error)
          }
        },
        (error) => {
          clearTimeout(timeout)
          reject(error)
        },
      )
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
        },
        // Opts this connection into the observe messages (CONTRACT D2).
        features: [...SDK_FEATURES],
      },
      action: { type: 'void' },
      dispatch: 'remote',
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
   *
   * @deprecated __helpers_create_stream is deprecated (iii-stream) and will be removed in an upcoming release. Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
   */
  __helpers_create_stream = <TData>(streamName: string, stream: IStream<TData>): void => {
    this.registerFunction(`stream::get(${streamName})`, stream.get.bind(stream))
    this.registerFunction(`stream::set(${streamName})`, stream.set.bind(stream))
    this.registerFunction(`stream::delete(${streamName})`, stream.delete.bind(stream))
    this.registerFunction(`stream::list(${streamName})`, stream.list.bind(stream))
    this.registerFunction(`stream::list_groups(${streamName})`, stream.listGroups.bind(stream))
  }

  /**
   * The current WebSocket connection state. `'failed'` is terminal: it follows
   * a fatal registration rejection (see {@link getFatalError}). Mirrors the
   * Python/Rust SDKs' `get_connection_state()`.
   */
  getConnectionState = (): IIIConnectionState => this.connectionState

  /**
   * Engine address this worker resolved to: the explicit argument, else
   * `III_URL`, else {@link DEFAULT_ENGINE_URL}. Mirrors the Rust SDK's
   * `address()` and the Python SDK's `get_address()`.
   */
  getAddress = (): string => this.address

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
    this.removeProcessMarker()

    // Last chance for buffered reports and counters to reach the engine.
    try {
      this.reporter.flush()
      this.reporter.sendStats()
    } catch (error) {
      this.logError('Failed to flush observed invocations on shutdown', error)
    }
    this.reporter.stop()

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
      // Match the engine's inbound message bound; outbound envelopes may be fragmented up to the same size.
      maxPayload: MAX_JSON_MESSAGE_BYTES,
      headers: resolveHeaders(this.options?.headers),
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
    // Local dispatch and the compiled observer sets survive; what was sent on
    // this connection is replayed on the next one.
    this.registeredOnConnection = false

    this.setConnectionState('disconnected')
    this.stopMetricsReporting()
    this.scheduleReconnect()
  }

  private onSocketOpen(): void {
    this.clearReconnectTimeout()
    this.reconnectAttempt = 0
    this.setConnectionState('connected')
    this.connectionEpoch++
    this.registeredOnConnection = false

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
      this.replayRegistration(MessageType.RegisterTriggerType, message)
    })
    this.functions.forEach(({ message }) => {
      this.replayRegistration(MessageType.RegisterFunction, message)
    })
    this.triggers.forEach((trigger) => {
      this.replayRegistration(MessageType.RegisterTrigger, trigger)
    })
    // Observers follow the functions (so local targets exist first), once
    // `WorkerRegistered` says whether this engine supports them.

    // Optimized: swap with empty array instead of splice
    const pending = this.messagesToSend
    this.messagesToSend = []
    for (const message of pending) {
      // Observe frames wait for this engine's features (WorkerRegistered).
      if (OBSERVE_FAMILY.has(message.type as MessageType)) {
        this.observeQueue.push(message)
        continue
      }
      if (
        message.type === MessageType.InvokeFunction &&
        typeof message.invocation_id === 'string' &&
        !this.invocations.has(message.invocation_id)
      ) {
        continue
      }
      try {
        this.sendMessageRaw(JSON.stringify(message))
      } catch (error) {
        if (!(error instanceof InvocationError) || error.code !== 'payload_too_large') throw error
        this.logError('Queued JSON envelope rejected', error)
      }
    }
  }

  private snapshotRegistration<T extends object>(message: T): T {
    // Match JSON.stringify's existing wire semantics (toJSON, undefined, etc.),
    // while retaining no caller-owned nested objects and never copying handlers.
    return JSON.parse(JSON.stringify(message)) as T
  }

  private replayRegistration(type: MessageType, message: Omit<IIIMessage, 'message_type'>): void {
    try {
      this.sendMessage(type, message, true)
    } catch (error) {
      if (!(error instanceof InvocationError) || error.code !== 'payload_too_large') throw error
      this.logError('Registration JSON envelope rejected', error)
    }
  }

  private isOpen(): boolean {
    return this.ws?.readyState === WebSocket.OPEN
  }

  // Classify the complete serialized envelope against the inclusive message limit.
  private prepareJson(message: Record<string, unknown>): string {
    const data = JSON.stringify(message)
    const size = Buffer.byteLength(data, 'utf8')
    if (size <= MAX_JSON_MESSAGE_BYTES) return data
    const detail = `Serialized JSON envelope is ${size} bytes; limit is ${MAX_JSON_MESSAGE_BYTES} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size`
    const error = new InvocationError({ code: 'payload_too_large', message: detail })
    if (message.type === MessageType.InvocationResult) {
      // The wire body carries the bare message: the caller's InvocationError adds the `code: ` prefix.
      const fallback = JSON.stringify({
        type: MessageType.InvocationResult,
        invocation_id: message.invocation_id,
        function_id: message.function_id,
        error: { code: error.code, message: detail },
      })
      if (Buffer.byteLength(fallback, 'utf8') <= MAX_JSON_MESSAGE_BYTES) return fallback
    }
    if (typeof message.invocation_id === 'string') {
      const pending = this.invocations.get(message.invocation_id)
      this.invocations.delete(message.invocation_id)
      pending?.reject(error)
    }
    throw error
  }

  private sendMessageRaw(data: string): void {
    // Reattach and queue flush bypass sendMessage.
    if (Buffer.byteLength(data, 'utf8') > MAX_JSON_MESSAGE_BYTES) {
      data = this.prepareJson(JSON.parse(data))
    }
    if (this.ws && this.isOpen()) {
      let offset = 0
      try {
        const size = Buffer.byteLength(data, 'utf8')
        if (size <= MAX_JSON_FRAME_BYTES) {
          this.ws.send(data, (err) => {
            if (err) {
              this.logError('Failed to send message', err)
            }
          })
          return
        }

        const bytes = Buffer.from(data, 'utf8')
        // Keep this loop synchronous: ws queues sends in call order, so no data
        // frame from another message can appear between these fragments.
        for (; offset < bytes.length; offset += MAX_JSON_FRAGMENT_BYTES) {
          const end = Math.min(offset + MAX_JSON_FRAGMENT_BYTES, bytes.length)
          this.ws.send(bytes.subarray(offset, end), { binary: false, fin: end === bytes.length }, (err) => {
            if (err) {
              this.logError('Failed to send message', err)
            }
          })
        }
      } catch (error) {
        if (offset > 0) this.ws?.terminate()
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
    const data = this.prepareJson(wireMessage)
    if (this.isOpen()) {
      this.sendMessageRaw(data)
    } else if (!skipIfClosed) {
      this.messagesToSend.push(JSON.parse(data))
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
   *   other functions. Non-fatal -- log a warning and continue.
   * - Any other code: treated as fatal (safe default).
   */
  private onRegistrationRejected(init: {
    code: string
    namespace: string
    worker_name?: string
    function_id?: string
    owner_worker_id: string
  }): void {
    if (init.code === FUNCTION_NAMESPACE_CONFLICT) {
      // The engine refused it: no local dispatch, no observers for it here.
      if (init.function_id !== undefined)
        this.functions.get(init.function_id)?.observe?.onRegistrationRejected(this.connectionEpoch)
      this.logWarn(
        `Function registration rejected: function "${init.function_id ?? '<unknown>'}" in namespace "${init.namespace}" is already exported by worker ${init.owner_worker_id}. The worker stays connected and keeps serving its other functions.`,
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
    const message = typeof error === 'string' ? error : (JSON.stringify(error) ?? String(error))
    return new InvocationError({
      code: 'UNKNOWN',
      message,
      function_id,
    })
  }

  /**
   * Replaces channel refs with readers / writers. Copy-on-write: a value
   * without refs is returned as it is. Only arrays and plain objects are
   * walked, so a locally dispatched payload keeps its `Date`s, `Map`s and
   * class instances.
   */
  private resolveChannelValue(value: unknown): unknown {
    if (isChannelRef(value)) {
      return value.direction === 'read'
        ? new ChannelReader(this.address, value, this.options?.headers)
        : new ChannelWriter(this.address, value, this.options?.headers)
    }
    if (Array.isArray(value)) {
      let out: unknown[] | undefined
      for (let i = 0; i < value.length; i++) {
        const resolved = this.resolveChannelValue(value[i])
        if (resolved !== value[i]) {
          out ??= value.slice()
          out[i] = resolved
        }
      }
      return out ?? value
    }
    if (value !== null && typeof value === 'object') {
      const proto = Object.getPrototypeOf(value)
      if (proto !== Object.prototype && proto !== null) return value
      let out: Record<string, unknown> | undefined
      for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
        const resolved = this.resolveChannelValue(v)
        if (resolved !== v) {
          out ??= { ...(value as Record<string, unknown>) }
          out[k] = resolved
        }
      }
      return out ?? value
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
    observeFields: { caller?: CallerWire; observe_depth?: number; activation?: ActivationWire } = {},
  ): Promise<unknown> {
    const fn = this.functions.get(function_id)
    const getResponseTraceparent = () => injectTraceparent() ?? traceparent
    const getResponseBaggage = () => injectBaggage() ?? baggage

    const resolvedInput = this.resolveChannelValue(input) as TInput

    if (fn?.handler) {
      const handler = fn.handler
      // The invocation context every handler sees (`currentInvocation()`), and
      // what the calls it makes carry as their caller.
      const ctx: ActiveInvocation = {
        function_id,
        invocation_id: invocation_id ?? crypto.randomUUID(),
        observe_depth: observeFields.observe_depth ?? 0,
        metadata,
        traceparent,
        baggage,
      }
      if (observeFields.caller) ctx.caller = observeFields.caller
      if (observeFields.activation) ctx.activation = observeFields.activation
      const run = () => runInContext(ctx, () => handler(resolvedInput, metadata, traceparent, baggage))

      // Fire-and-forget (`void`, no invocation id): never answered. Activations
      // carry an id and are answered like any call (CONTRACT D16).
      if (!invocation_id) {
        try {
          await run()
        } catch (error) {
          this.logError(`Error invoking function ${function_id}`, error)
        }
        return
      }

      try {
        const result = await run()
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

  private onTriggerRegistrationResult(message: {
    id: string
    trigger_type?: string
    type?: string
    function_id: string
    error?: { code: string; message: string; stacktrace?: string }
  }): void {
    if (!message.error) return
    const triggerType = message.trigger_type ?? message.type ?? ''
    console.error(`[iii] Trigger registration failed for "${message.id}" (${triggerType}): ${message.error.message}`)
  }

  /**
   * `WorkerRegistered` told this connection what the engine supports. With
   * `observe.v1` the observers are replayed (after the functions, sent on
   * open) and held reports flushed; without it `observe()` fails with
   * `UNSUPPORTED` and observe hooks are inactive.
   */
  private onEngineFeatures(features: string[]): void {
    this.engineFeatures = new Set(features)
    this.registeredOnConnection = true
    const queued = this.observeQueue
    this.observeQueue = []
    if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      for (const message of queued) this.sendMessageRaw(JSON.stringify(message))
      for (const handle of this.observers.values()) this.sendObserve(handle)
      this.reporter.flush()
      return
    }
    for (const handle of this.observers.values()) handle.setState('error', UNSUPPORTED_REASON)
    for (const [functionId, fn] of this.functions) {
      if (fn.observe?.needsObserve) this.warnHooksInactive(functionId)
    }
    this.reporter.clear()
  }

  private onObserverState(id: string, wireState: string, reason?: ErrorBody): void {
    const handle = this.observers.get(id)
    if (!handle) return
    const state = toObserverState(wireState)
    handle.setState(state, reason ? { code: reason.code, message: reason.message } : undefined)
    if (state === 'disposed') this.observers.delete(id)
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
      const { invocation_id, function_id, data, metadata, traceparent, baggage, caller, observe_depth, activation } =
        decodeInvokeFunction(message)
      this.onInvokeFunction(invocation_id, function_id, data, metadata, traceparent, baggage, {
        caller,
        observe_depth,
        activation,
      })
    } else if (msgType === MessageType.FunctionRegistered) {
      const msg = decodeFunctionRegistered(message)
      // Its reports held across a reconnect are flushed once the set is acked.
      this.functions.get(msg.id)?.observe?.onFunctionRegistered(msg, this.connectionEpoch)
    } else if (msgType === MessageType.ObserverAdded) {
      const msg = decodeObserverAdded(message)
      this.functions.get(msg.function_id)?.observe?.onObserverAdded(msg)
    } else if (msgType === MessageType.ObserverRemoved) {
      const msg = decodeObserverRemoved(message)
      this.functions.get(msg.function_id)?.observe?.onObserverRemoved(msg)
    } else if (msgType === MessageType.ObserveResult) {
      const msg = decodeObserveResult(message)
      if (msg.warnings && msg.warnings.length > 0) {
        this.logWarn(`Observer ${msg.id}: ${msg.warnings.join('; ')}`)
      }
      this.onObserverState(msg.id, msg.state, msg.error)
    } else if (msgType === MessageType.ObserverStateChanged) {
      const msg = decodeObserverStateChanged(message)
      this.onObserverState(msg.id, msg.state, msg.reason)
    } else if (msgType === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(
        message as {
          trigger_type: string
          id: string
          function_id: string
          config: unknown
          metadata?: Record<string, unknown>
        },
      )
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
        message as {
          id: string
          trigger_type?: string
          type?: string
          function_id: string
          error?: { code: string; message: string; stacktrace?: string }
        },
      )
    } else if (msgType === MessageType.WorkerRegistered) {
      const { worker_id, reattach_token, features } = decodeWorkerRegistered(message)
      this.workerId = worker_id
      this.reattachToken = reattach_token
      console.debug('[iii] Worker registered with ID:', worker_id)
      this.startMetricsReporting()
      this.onEngineFeatures(features ?? [])
    } else if (msgType === MessageType.RegistrationRejected) {
      const { code, namespace, worker_name, function_id, owner_worker_id } = message as RegistrationRejectedMessage
      this.onRegistrationRejected({ code, namespace, worker_name, function_id, owner_worker_id })
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
   * Requires the `queue` worker in `worker-compose.yaml`.
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
 * @param address - WebSocket URL of the III engine. Omit to resolve it from
 *   `process.env.III_URL`, falling back to {@link DEFAULT_ENGINE_URL}.
 * @param options - Optional {@link InitOptions} for worker name, timeouts, reconnection, and OTel.
 * @returns A connected {@link IIIClient} instance.
 *
 * @example
 * ```typescript
 * import { registerWorker } from 'iii-sdk'
 *
 * // Address from III_URL, set by whatever supervisor spawned this worker.
 * const worker = registerWorker()
 *
 * // Or explicitly, which always wins over the environment.
 * const other = registerWorker('ws://localhost:49134', { workerName: 'my-worker' })
 * ```
 */
export const registerWorker = (address?: string, options?: InitOptions): IIIClient =>
  new Sdk(resolveAddress(address), options)
