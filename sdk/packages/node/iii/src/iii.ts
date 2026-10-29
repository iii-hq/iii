import { createRequire } from 'node:module'
import * as os from 'node:os'
import { context, trace } from '@opentelemetry/api'
import { type Data, WebSocket } from 'ws'
import { ChannelReader, ChannelWriter } from './channels'
import { InvocationError, isErrorBody, RegistrationRejectedError } from './errors'
import { compileFilter, FilterError } from './filter'
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
import { announceProcess } from './process-marker'

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
  type CallAction as CallActionType,
  type CallerWire,
  type CallRequest,
  type ErrorBody,
  FEATURE_OBSERVE_V1,
  FEATURE_OBSERVE_V2,
  type IIIMessage,
  type InvocationResultMessage,
  type JsonValue,
  MessageType,
  type RegisterFunctionMessage,
  SDK_FEATURES,
  type StreamChannelRef,
} from './iii-types'
import {
  type ActiveInvocation,
  currentContext,
  MAX_LOCAL_DEPTH,
  publicContext,
  runInContext,
} from './invocation-context'
import {
  Delivery,
  type FunctionHooks,
  FunctionObservers,
  handlerErrorBody,
  OBSERVE_HOOK_TIMEOUT_MS,
  type ObserveHost,
  type ObserverImpl,
} from './observable'
import type {
  AddedHook,
  Call,
  DeliverHook,
  InvocationContext,
  MatchHook,
  Observer,
  ObserverHandle,
  ObserverOptions,
  ObserveSource,
  RemovedHook,
  Target,
} from './observe-types'
import {
  buildInvocationResult,
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
  decodeRegistrationRejected,
  decodeWorkerRegistered,
  stripCaller,
} from './observe-wire'
import { ObserverHandleImpl, toObserverState } from './observer-handle'
import { Reporter, type ReporterLimits } from './reporter'
import type { IStream } from './stream'
import type {
  FunctionRef,
  HandlerOutcome,
  IIIClient,
  Invocation,
  RegisterFunctionOptions,
  RemoteFunctionData,
  RemoteFunctionHandler,
} from './types'
import { detectProjectName, freezePlain, isChannelRef, resolveHeaders } from './utils'

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

/**
 * `registrationrejected` codes that refuse one function only (decisions 1,
 * 11): its `args_format` is not a valid JSON Schema, or the engine does not
 * support what it declares. The connection stays open -- non-fatal.
 */
const FUNCTION_REJECTIONS: ReadonlySet<string> = new Set(['INVALID_ARGS_FORMAT', 'UNSUPPORTED'])

/** A standby function retries its registration this often (decision 21), ±20 %. */
const STANDBY_RETRY_MS = 1000

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
  source = 'CallRequest.namespace',
): string | undefined {
  const namespace = callNamespace(explicit, worker, source)
  return explicit === undefined && (functionId.startsWith('engine::') || functionId.startsWith('configuration::'))
    ? 'default'
    : namespace
}

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

function resolveNamespace(optionNamespace?: string): string | undefined {
  // III_NAMESPACE carries the namespace for managed workers (set by iii-worker
  // at spawn), mirroring III_WORKER_NAME. An explicit option wins; otherwise
  // the env var provides the managed identity. Absent in both means undefined
  // -- the engine applies its `default` namespace when none is on the wire.
  //
  // A declared-but-blank namespace throws rather than reading as "no
  // namespace". The two mean opposite things: absent asks for the engine's
  // default, blank names a namespace and gives nothing to name it with. Read
  // as absent, the worker registers in `default`, and every call and observer it
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
   * everything the worker does afterwards follows it: a `call` resolves its
   * target here, and an `observe` its source and target, unless the call
   * names another namespace. One declaration places the whole worker.
   */
  namespace?: string
  /**
   * One-line, human/LLM-readable summary of what this worker does.
   * Surfaces in `engine::workers::list` / `engine::workers::info`.
   */
  workerDescription?: string
  /** Enable worker metrics via OpenTelemetry. Defaults to `true`. */
  enableMetricsReporting?: boolean
  /** Default timeout for `worker.call()` invocations in milliseconds. Defaults to `30000`. */
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
  private observerHandles = new Map<string, ObserverHandleImpl>()
  // The engine's features (`WorkerRegistered.features`), kept across a
  // disconnect until the next `WorkerRegistered` replaces them.
  private engineFeatures: ReadonlySet<string> = new Set()
  // `WorkerRegistered` arrived on the current connection.
  private registeredOnConnection = false
  // Bumped on every connection: `functionregistered` confirms a function on one.
  private connectionEpoch = 0
  // Observe-family frames that wait for the next connection's features.
  private observeQueue: Record<string, unknown>[] = []
  // Observer hooks of every function this worker hosts.
  private workerHooks: { added?: AddedHook; removed?: RemovedHook } = {}
  // Observer hooks by function id: they outlive a registration of the id.
  private functionHooks = new Map<
    string,
    { added?: AddedHook; removed?: RemovedHook; match?: MatchHook; deliver?: DeliverHook }
  >()
  private hooksInactiveWarned = new Set<string>()
  private unsupportedWarned = new Set<string>()
  private unregisteredHooksWarned = new Set<string>()
  // Standby functions waiting for their id to be free, by function id.
  private standbyTimers = new Map<string, NodeJS.Timeout>()
  // Decision 22: a call to a private function this worker hosts freezes its input and result.
  private readonly freezePrivate = process.env.NODE_ENV === 'development'
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
      sendAck: (ack) => this.sendMessage(MessageType.ObserverAck, buildObserverAck(ack), true),
      reporter: this.reporter,
      hooks: (functionId) => this.hooksOf(functionId),
      logError: (message, error) => this.logError(message, error),
      logWarn: (message) => this.logWarn(message),
      hookTimeoutMs: () => options?.observe?.hookTimeoutMs ?? OBSERVE_HOOK_TIMEOUT_MS,
      observeV2: () => this.engineFeatures.has(FEATURE_OBSERVE_V2),
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

  /**
   * Registers a function with the engine. The `functionId` is the unique
   * identifier callers and observers name. The handler's return value is what
   * the function's observers observe.
   *
   * Pass a handler for local execution, or an {@link HttpInvocationConfig}
   * for HTTP-invoked functions (Lambda, Cloudflare Workers, etc.).
   *
   * A function that sets `args_format`, `private: true`, `observable: false`
   * or an `onObserverDeliver` hook needs an engine with `observe.v2`: it is
   * registered once the connection's engine advertised it, and against an
   * older engine it stays unregistered (logged once, `UNSUPPORTED`).
   *
   * @param functionId - Unique function identifier.
   * @param handlerOrInvocation - Handler or HTTP invocation config.
   * @param options - Optional {@link RegisterFunctionOptions}.
   * @returns A {@link FunctionRef}: the hook methods with the id filled in, and `unregister()`.
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
  // biome-ignore lint/suspicious/noExplicitAny: the handler's types default to any so untyped handlers type-check
  registerFunction = <I = any, O = any>(
    functionId: string,
    handlerOrInvocation: RemoteFunctionHandler<I, O> | HttpInvocationConfig,
    options?: RegisterFunctionOptions,
  ): FunctionRef<I, O> => {
    if (!functionId || functionId.trim() === '') {
      throw new Error('id is required')
    }
    if (this.functions.has(functionId)) {
      throw new Error(`function id already registered: ${functionId}`)
    }

    // `standby` and `localDispatch` are this SDK's alone; `private: false` and
    // `observable: true` are the defaults and are not sent.
    const { standby, localDispatch, private: isPrivate, observable, ...wireOptions } = options ?? {}
    const registration = {
      ...wireOptions,
      ...(isPrivate === true ? { private: true } : {}),
      ...(observable === false ? { observable: false } : {}),
    }

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
    // Refused here when too large, before anything is retained.
    const frame = this.prepareJson(this.toWireFormat(MessageType.RegisterFunction, fullMessage))

    const observe = new FunctionObservers(functionId, localDispatch !== false, isHandler, this.observeHost)
    const fn: RemoteFunctionData = { message: fullMessage, observe, standby: standby === true }
    if (isHandler) {
      fn.handler = this.wrapHandler(functionId, handlerOrInvocation as RemoteFunctionHandler)
    }
    this.functions.set(functionId, fn)
    if (this.mayRegister(functionId, fn)) {
      this.sendMessageRaw(frame)
      fn.sentEpoch = this.connectionEpoch
    }
    if (
      this.registeredOnConnection &&
      !this.engineFeatures.has(FEATURE_OBSERVE_V1) &&
      this.hasObserverHooks(functionId)
    ) {
      this.warnHooksInactive(functionId)
    }

    const ref: FunctionRef<I, O> = {
      id: functionId,
      onObserverAdded: (hook) => {
        this.onObserverAdded(functionId, hook)
        return ref
      },
      onObserverRemoved: (hook) => {
        this.onObserverRemoved(functionId, hook)
        return ref
      },
      onObserverMatch: (hook: MatchHook<I, O> | null) => {
        this.onObserverMatch(functionId, hook)
        return ref
      },
      onObserverDeliver: (hook) => {
        this.onObserverDeliver(functionId, hook)
        return ref
      },
      observers: () => this.observers(functionId),
      isObserved: (call) => this.isObserved(functionId, call as Partial<Call>),
      unregister: () => {
        // A stale ref (the id was unregistered and registered again) must not
        // touch the registration that replaced it: nothing to do.
        if (this.functions.get(functionId) !== fn) return
        this.sendMessage(MessageType.UnregisterFunction, { id: functionId }, true)
        this.functions.delete(functionId)
        this.stopStandby(functionId)
        observe.dispose()
      },
    }
    return ref
  }

  /**
   * The user handler wrapped for the SDK: its `execute <fn>` span, the payload
   * events, and `settle`, which runs inside them (the call's delivery).
   */
  private wrapHandler(functionId: string, handler: RemoteFunctionHandler): RemoteFunctionData['handler'] {
    return async (input, metadata, traceparent, baggage, settle) => {
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
        let outcome: HandlerOutcome
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
          outcome = { ok: true, value: result }
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
          outcome = { ok: false, error: err }
        }
        if (settle) return settle(outcome)
        if (outcome.ok) return outcome.value
        throw outcome.error
      }

      if (getTracer()) {
        const parentContext = extractContext(traceparent, baggage)

        // INTERNAL and named `execute` (not `call`): the engine
        // suppresses its own `call <fn>` span for worker-routed functions
        // (this span is the canonical one for the invocation) but still
        // emits engine-side wrappers like `call <fn>` for builtins. Reusing
        // those names would read as duplicate engine spans under the
        // worker's service. `execute` is unique, so the worker handler span
        // reads as a clean internal child (and is collapsible by a single rule).
        return context.with(parentContext, () =>
          withSpan(`execute ${functionId}`, { kind: SpanKind.INTERNAL }, async () => await runHandler()),
        )
      }

      const traceId = crypto.randomUUID().replace(/-/g, '')
      const spanId = crypto.randomUUID().replace(/-/g, '').slice(0, 16)
      const syntheticSpan = trace.wrapSpanContext({ traceId, spanId, traceFlags: 1 })

      return context.with(trace.setSpan(context.active(), syntheticSpan), async () => await runHandler())
    }
  }

  /** What a function needs `observe.v2` for (decision 18): `args_format`, `private`, `observable: false`, a deliver hook. */
  private needsObserveV2(functionId: string, fn: RemoteFunctionData): boolean {
    const { message } = fn
    return (
      message.args_format !== undefined ||
      message.private === true ||
      message.observable === false ||
      this.functionHooks.get(functionId)?.deliver !== undefined
    )
  }

  /**
   * Whether the function's `registerfunction` may go out on the current
   * connection now. One that needs `observe.v2` waits for this connection's
   * `WorkerRegistered`, and stays unregistered (logged once, `UNSUPPORTED`)
   * when the engine does not advertise it (decision 41).
   */
  private mayRegister(functionId: string, fn: RemoteFunctionData): boolean {
    if (!this.isOpen()) return false
    if (!this.needsObserveV2(functionId, fn)) return true
    if (!this.registeredOnConnection) return false
    if (this.engineFeatures.has(FEATURE_OBSERVE_V2)) return true
    this.warnUnsupported(functionId)
    return false
  }

  /** Sends the registrations that may go out now and have not on this connection. */
  private sendRegistrations(): void {
    for (const [functionId, fn] of this.functions) {
      if (fn.sentEpoch === this.connectionEpoch || !this.mayRegister(functionId, fn)) continue
      this.replayRegistration(MessageType.RegisterFunction, fn.message)
      fn.sentEpoch = this.connectionEpoch
    }
  }

  /**
   * A deliver hook changed whether the function needs `observe.v2`: against
   * an engine without it, a registered function is unregistered (and one
   * held back is registered once it no longer needs it).
   */
  private reconcileRegistration(functionId: string): void {
    const fn = this.functions.get(functionId)
    if (!fn || !this.isOpen() || !this.registeredOnConnection) return
    const sent = fn.sentEpoch === this.connectionEpoch
    const allowed = this.engineFeatures.has(FEATURE_OBSERVE_V2) || !this.needsObserveV2(functionId, fn)
    if (sent && !allowed) {
      this.sendMessage(MessageType.UnregisterFunction, { id: functionId }, true)
      fn.sentEpoch = undefined
      fn.observe.onRegistrationRejected(this.connectionEpoch)
      this.warnUnsupported(functionId)
    } else if (!sent && allowed) {
      this.replayRegistration(MessageType.RegisterFunction, fn.message)
      fn.sentEpoch = this.connectionEpoch
    }
  }

  private warnUnsupported(functionId: string): void {
    if (this.unsupportedWarned.has(functionId)) return
    this.unsupportedWarned.add(functionId)
    this.logWarn(
      `UNSUPPORTED: function "${functionId}" needs observe.v2 (args_format, private, observable: false or onObserverDeliver), which this engine does not advertise; it stays unregistered until the worker connects to an engine with observe.v2.`,
    )
  }

  /** The hooks that apply to one function now: the worker's, and those set for its id. */
  private hooksOf(functionId: string): FunctionHooks {
    const own = this.functionHooks.get(functionId)
    return {
      workerAdded: this.workerHooks.added,
      workerRemoved: this.workerHooks.removed,
      added: own?.added,
      removed: own?.removed,
      match: own?.match,
      deliver: own?.deliver,
    }
  }

  private hasObserverHooks(functionId: string): boolean {
    const own = this.functionHooks.get(functionId)
    return !!(
      this.workerHooks.added ||
      this.workerHooks.removed ||
      own?.added ||
      own?.removed ||
      own?.match ||
      own?.deliver
    )
  }

  private setFunctionHook<K extends 'added' | 'removed' | 'match' | 'deliver'>(
    functionId: string,
    kind: K,
    hook: { added: AddedHook; removed: RemovedHook; match: MatchHook; deliver: DeliverHook }[K] | null,
  ): void {
    if (typeof functionId !== 'string' || functionId.trim() === '') {
      throw new TypeError('the function id of an observer hook is required')
    }
    const keys =
      kind === 'match' &&
      hook !== null &&
      typeof hook === 'object' &&
      typeof (hook as { observer?: unknown }).observer === 'function' &&
      typeof (hook as { call?: unknown }).call === 'function'
    if (hook !== null && typeof hook !== 'function' && !keys) {
      throw new TypeError(
        kind === 'match'
          ? `the match hook of ${functionId} must be a predicate, { observer, call } key functions, or null to remove it`
          : `the ${kind} hook of ${functionId} must be a function, or null to remove it`,
      )
    }
    const hooks = { ...this.functionHooks.get(functionId), [kind]: hook ?? undefined }
    if (hooks.added || hooks.removed || hooks.match || hooks.deliver) this.functionHooks.set(functionId, hooks)
    else this.functionHooks.delete(functionId)
    const fn = this.functions.get(functionId)
    if (!fn) return
    if (kind === 'added' && hook) fn.observe.catchUp('fn')
    if (kind === 'match') fn.observe.matchChanged()
    if (kind === 'deliver') this.reconcileRegistration(functionId)
  }

  private setWorkerHook<K extends 'added' | 'removed'>(
    kind: K,
    hook: { added: AddedHook; removed: RemovedHook }[K] | null,
  ): void {
    if (hook !== null && typeof hook !== 'function') {
      throw new TypeError(`the worker's ${kind} hook must be a function, or null to remove it`)
    }
    this.workerHooks = { ...this.workerHooks, [kind]: hook ?? undefined }
    if (kind === 'added' && hook) {
      for (const fn of this.functions.values()) fn.observe.catchUp('worker')
    }
  }

  /**
   * Runs `hook` when an observer of any function this worker hosts is added,
   * before the SDK acks it; with a function id, for that function only. When
   * both are set, the worker's runs first. Throw to reject the observer (an
   * {@link ObserveRejection} sets the code). `null` removes the hook.
   */
  onObserverAdded: IIIClient['onObserverAdded'] = (a: string | AddedHook | null, b?: AddedHook | null) => {
    if (typeof a === 'string') this.setFunctionHook(a, 'added', b as AddedHook | null)
    else this.setWorkerHook('added', a)
    return this
  }

  /**
   * Runs `hook` once for every accepted observer when it is removed; with a
   * function id, for that function only. When both are set, the function's
   * runs first. `null` removes the hook.
   */
  onObserverRemoved: IIIClient['onObserverRemoved'] = (a: string | RemovedHook | null, b?: RemovedHook | null) => {
    if (typeof a === 'string') this.setFunctionHook(a, 'removed', b as RemovedHook | null)
    else this.setWorkerHook('removed', a)
    return this
  }

  /**
   * Says which observers want each call of `functionId`: a predicate
   * `(obs, call) => boolean`, or `{ observer, call }` key functions the SDK
   * indexes. iii's delivery (or the function's deliver hook) gets only the
   * observers that want the call. `null` removes the hook.
   */
  onObserverMatch: IIIClient['onObserverMatch'] = (functionId: string, hook: MatchHook | null): IIIClient => {
    this.setFunctionHook(functionId, 'match', hook)
    return this
  }

  /**
   * Delivers each call of `functionId` itself: the hook receives the call and
   * the observers that matched it, and what it returns (or throws) is what the
   * caller gets back. iii delivers nothing for that function. `null` removes
   * the hook.
   */
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverDeliver = <I = any, O = any, R = O>(functionId: string, hook: DeliverHook<I, O, R> | null): IIIClient => {
    this.setFunctionHook(functionId, 'deliver', hook as DeliverHook | null)
    return this
  }

  /** The active observers of a function this worker hosts (empty for any other). */
  observers = (functionId: string): readonly Observer[] => this.functions.get(functionId)?.observe.observers() ?? []

  /**
   * Whether a call of a function this worker hosts would reach anyone (see
   * {@link IIIClient.isObserved}).
   */
  isObserved = (functionId: string, call?: Partial<Call>): boolean =>
    this.functions.get(functionId)?.observe.isObserved(call) ?? false

  /**
   * Observes a function: every call of `source` that passes `source.filters`
   * runs `target`, a registered function. `source` is a function id, or an
   * {@link ObserveSource} with `args` for the worker that hosts the function.
   *
   * Returns synchronously, also before the connection is up; the observer is
   * replayed with the same id on every reconnect. Errors (`INVALID_ARGS`,
   * `INVALID_FILTER`, `FORBIDDEN`, `NOT_OBSERVABLE`, the host's rejection,
   * `UNSUPPORTED` against an engine without observe) show up as state `error`
   * and reject `ready()`.
   *
   * @param source - What is observed: a function id, or `function_id`,
   *   `args`, `filters`, and `on` / `capture` / `deliver`.
   * @param target - The function to invoke (registered first; until then the observer is `pending`).
   * @param options - Policy and metadata: `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * iii.observe('orders::create', 'search::reindex-order')
   *
   * iii.registerFunction('alerts::payment-failed', async ({ input, error }: Observed) => notify(input, error))
   * const failures = iii.observe(
   *   { function_id: 'payments::charge', on: 'error', filters: { 'input.amount': { $gte: 500 } } },
   *   'alerts::payment-failed',
   * )
   * await failures.ready()
   * // later
   * await failures.dispose()
   * ```
   */
  observe = (sourceOrId: string | ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle => {
    const source: ObserveSource = typeof sourceOrId === 'string' ? { function_id: sourceOrId } : sourceOrId
    if (!source || typeof source.function_id !== 'string' || source.function_id.trim() === '') {
      throw new Error('observe: source.function_id is required')
    }
    const targetId = typeof target === 'string' ? target : target?.function_id
    if (typeof targetId !== 'string' || targetId.trim() === '') {
      throw new Error('observe: target is required (the id of a registered function)')
    }
    const id = options?.id ?? crypto.randomUUID()
    if (this.observerHandles.has(id)) {
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
    this.observerHandles.set(id, handle)
    if (this.registeredOnConnection && this.isOpen()) {
      if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) this.sendObserve(handle)
      else handle.setState('error', UNSUPPORTED_REASON)
    }
    return handle
  }

  /**
   * The invocation the calling code runs in (in every handler, remote or
   * local, and in its function's deliver hook), or `undefined` outside any
   * handler.
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
    if (this.observerHandles.get(handle.id) === handle) this.observerHandles.delete(handle.id)
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
      `Function "${functionId}" has observer hooks, but the engine does not support observe (observe.v1): it is registered as a plain function and its observer hooks are inactive.`,
    )
  }

  /** Once the connection is registered: a hook set for an id this worker has not registered (a mistyped id?). */
  private warnUnregisteredHookIds(): void {
    for (const functionId of this.functionHooks.keys()) {
      if (this.functions.has(functionId) || this.unregisteredHooksWarned.has(functionId)) continue
      this.unregisteredHooksWarned.add(functionId)
      this.logWarn(
        `Observer hooks are set for "${functionId}", which this worker has not registered; check the function id.`,
      )
    }
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
    const result = await this.call<{ buffer_size?: number }, { writer: StreamChannelRef; reader: StreamChannelRef }>({
      function_id: 'engine::channels::create',
      payload: { buffer_size: bufferSize },
    })

    return {
      writer: new ChannelWriter(this.address, result.writer, this.options?.headers),
      reader: new ChannelReader(this.address, result.reader, this.options?.headers),
      writerRef: result.writer,
      readerRef: result.reader,
    }
  }

  /**
   * Invokes a function. The routing behavior and return type depend on the
   * `action` field of the request.
   *
   * | `action`                   | Behavior                                           | Return type              |
   * |----------------------------|----------------------------------------------------|--------------------------|
   * | _(none)_                   | Synchronous: waits for the function to return     | `Promise<TOutput>`       |
   * | `CallAction.Enqueue(...)`  | Async via named queue; engine acknowledges enqueue | `Promise<EnqueueResult>` |
   * | `CallAction.Void()`        | Fire-and-forget, no response                      | `Promise<undefined>`     |
   *
   * With `observer` instead of `function_id`, the call runs that observer's
   * target, with the observer's policy (`max_fires`, expiry,
   * `max_in_flight`), metadata and loop guard. Only the worker that hosts the
   * observed function may call through it (`NOT_FUNCTION_OWNER`); the
   * payload goes as given.
   *
   * @param request - The call request.
   * @param request.function_id - ID of the function to invoke.
   * @param request.observer - An observer of a function this worker hosts: its target is invoked.
   * @param request.payload - Payload to pass to the function.
   * @param request.action - Routing action. Omit for synchronous request/response.
   * @param request.timeoutMs - Override the default invocation timeout.
   * @param request.metadata - Optional per-invocation metadata (arbitrary JSON)
   *   surfaced to the target handler as its second argument.
   * @returns The result of the function invocation.
   *
   * @example
   * ```typescript
   * import { CallAction } from 'iii-sdk'
   *
   * // Synchronous
   * const result = await worker.call({ function_id: 'get-order', payload: { id: '123' } })
   *
   * // Enqueue
   * const { messageReceiptId } = await worker.call({
   *   function_id: 'payments::charge',
   *   payload: { orderId: '123', amount: 49.99 },
   *   action: CallAction.Enqueue({ queue: 'payment' }),
   * })
   *
   * // Fire-and-forget
   * worker.call({
   *   function_id: 'notifications::send',
   *   payload: { userId: '123' },
   *   action: CallAction.Void(),
   * })
   *
   * // Through an observer, from the function's deliver hook
   * worker.onObserverDeliver('http::on-request', async (call) => {
   *   const route = routes.match(call.input)
   *   return route ? worker.call({ observer: route.obs, payload: call.input }) : { status_code: 404 }
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  call = async <TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput> => {
    if (request.observer !== undefined) {
      const { observer, payload, action, timeoutMs } = request
      // The engine routes to the observer's stored target and is the one that
      // checks it (decision 13); no namespace travels.
      return this.invokeRemote<TOutput>({ function_id: observer.target, payload, action, timeoutMs }, undefined, {
        observer_id: observer.id,
      })
    }
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
        return this.invokeLocal<TOutput>(function_id, fn, request)
      }
    }
    return this.invokeRemote<TOutput>(request, namespace)
  }

  /**
   * Invokes a function using a request object.
   *
   * @deprecated Use {@link Sdk.call}, the same function.
   */
  trigger = this.call

  /** `caller` and `observe_depth` an outgoing call carries from the current handler (engines with `observe.v1`). */
  private outgoingObserveFields(): { caller?: CallerWire; observe_depth?: number } {
    const cur = currentContext()
    if (!cur || !this.engineFeatures.has(FEATURE_OBSERVE_V1)) return {}
    return { caller: callerFor(cur), observe_depth: cur.observe_depth }
  }

  /** The routed path: an `InvokeFunction` through the engine. */
  private invokeRemote<TOutput>(
    request: { function_id: string; payload: unknown; action?: CallActionType; timeoutMs?: number; metadata?: unknown },
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
   * The local path (sdk.md §4.3): runs the handler in-process inside a new
   * invocation context, then delivers the call (the function's deliver hook,
   * or iii's delivery to its observers); the same timeout and error shape as
   * the remote path. A call of a function nobody observes and without a
   * deliver hook puts nothing on the wire.
   *
   * The handler gets a copy of the payload and the caller a copy of the
   * result; a private function's input and result pass by reference
   * (frozen when `NODE_ENV` is `development`).
   */
  private invokeLocal<TOutput>(
    function_id: string,
    fn: RemoteFunctionData,
    request: { payload: unknown; action?: CallActionType; timeoutMs?: number; metadata?: unknown },
  ): Promise<TOutput> {
    const { observe } = fn
    const handler = fn.handler as NonNullable<RemoteFunctionData['handler']>
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
      localDepth: (parent?.localDepth ?? 0) + 1,
    }

    // The observers the call is delivered from: those active when it started.
    const snapshot = observe.compiled
    const activations = observe.activations
    const hook = this.functionHooks.get(function_id)?.deliver
    const delivering = hook !== undefined || snapshot.length > 0
    const byReference = fn.message.private === true
    let input: unknown
    let metadata: unknown
    let callInput: unknown
    let callMetadata: unknown
    try {
      if (byReference) {
        // Nobody else sees either value: neither is copied, and both are read-only.
        if (this.freezePrivate) {
          freezePlain(request.payload)
          freezePlain(request.metadata)
        }
        callInput = request.payload
        callMetadata = request.metadata
        input = this.resolveChannelValue(request.payload)
        metadata = request.metadata
      } else {
        // What the delivery sees is the payload and metadata as the caller passed
        // them when the call started, not what the handler or the caller made of
        // them since. Not copied when nothing is delivered.
        if (delivering) {
          callInput = cloneValue(request.payload)
          callMetadata = request.metadata === undefined ? undefined : cloneValue(request.metadata)
        }
        input = this.resolveChannelValue(cloneValue(request.payload))
        metadata = request.metadata === undefined ? undefined : cloneValue(request.metadata)
      }
    } catch (error) {
      return Promise.reject(error)
    }

    const startedAt = Date.now()
    const t0 = performance.now()
    let delivery: Delivery | undefined
    const settle = (outcome: HandlerOutcome): unknown => {
      const duration = performance.now() - t0
      this.reporter.recordLocal(function_id, outcome.ok, duration)
      if (!delivering) {
        if (outcome.ok) return outcome.value
        throw outcome.error
      }
      delivery = new Delivery(observe, {
        call: this.callOf(function_id, ctx.invocation_id, outcome, callInput, ctx.caller, callMetadata),
        thrown: outcome.ok ? undefined : outcome.error,
        snapshot,
        activations,
        started_at_ms: startedAt,
        duration_ms: duration,
        observe_depth: ctx.observe_depth,
        traceparent,
        baggage,
        routed: false,
      })
      return delivery.run(hook)
    }

    const done = runInContext(ctx, () => handler(input, metadata as JsonValue, traceparent, baggage, settle)).then(
      (output) => output,
      (error) => {
        throw new InvocationError({ ...(delivery ? delivery.errorBody(error) : handlerErrorBody(error)), function_id })
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
            if (!byReference) resolve(cloneValue(output) as TOutput)
            else resolve((this.freezePrivate ? freezePlain(output) : output) as TOutput)
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

  /** The {@link Call} a finished handler made: what it was called with, and what it returned or threw. */
  private callOf(
    function_id: string,
    invocation_id: string,
    outcome: HandlerOutcome,
    input: unknown,
    caller: CallerWire | undefined,
    metadata: unknown,
  ): Call {
    const call: Call = { function_id, invocation_id, status: outcome.ok ? 'success' : 'error', input }
    if (outcome.ok) call.output = outcome.value
    else call.error = handlerErrorBody(outcome.error)
    if (caller !== undefined) call.caller = caller
    if (metadata !== undefined) call.metadata = metadata as Record<string, unknown>
    return call
  }

  private registerWorkerMetadata(): void {
    const telemetryOpts = this.options?.telemetry
    const language =
      telemetryOpts?.language ?? Intl.DateTimeFormat().resolvedOptions().locale ?? process.env.LANG?.split('.')[0]

    this.call({
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
    // this connection is replayed on the next one (standby functions too).
    this.registeredOnConnection = false
    for (const functionId of [...this.standbyTimers.keys()]) this.stopStandby(functionId)

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

    // Functions that need observe.v2 wait for `WorkerRegistered`, and so do
    // the observers, which follow the functions (so local targets exist first).
    this.sendRegistrations()

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

  private logInfo(message: string): void {
    const otelLogger = getLogger()
    if (otelLogger) {
      otelLogger.emit({
        severityNumber: SeverityNumber.INFO,
        body: `[iii] ${message}`,
      })
    } else {
      console.info(`[iii] ${message}`)
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
   *   other functions. Non-fatal -- log a warning and continue. A `standby`
   *   function retries its registration until the id is free.
   * - {@link FUNCTION_REJECTIONS} on one function: only that function is
   *   refused. Non-fatal -- log an error and continue.
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
      const fn = init.function_id !== undefined ? this.functions.get(init.function_id) : undefined
      // The engine refused it: no local dispatch, no observers for it here.
      fn?.observe.onRegistrationRejected(this.connectionEpoch)
      if (fn?.standby && init.function_id !== undefined) {
        this.standBy(init.function_id, fn, init.owner_worker_id)
        return
      }
      this.logWarn(
        `Function registration rejected: function "${init.function_id ?? '<unknown>'}" in namespace "${init.namespace}" is already exported by worker ${init.owner_worker_id}. The worker stays connected and keeps serving its other functions.`,
      )
      return
    }
    if (FUNCTION_REJECTIONS.has(init.code) && init.function_id !== undefined) {
      this.functions.get(init.function_id)?.observe.onRegistrationRejected(this.connectionEpoch)
      this.logError(
        `Function registration rejected with ${init.code}: function "${init.function_id}" in namespace "${init.namespace}" is not registered. The worker stays connected and keeps serving its other functions.`,
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

  /**
   * A `standby` function whose id another connection hosts (decision 21): its
   * registration is retried every second (±20 %) until the engine accepts it
   * (`functionregistered`, whose observers then run their hooks here) or the
   * function is unregistered.
   */
  private standBy(functionId: string, fn: RemoteFunctionData, owner: string): void {
    if (this.standbyTimers.has(functionId)) return
    if (!fn.standingBy) {
      this.logInfo(`Function "${functionId}" is hosted by worker ${owner}; standing by to take it over when it leaves.`)
    }
    fn.standingBy = true
    const delay = Math.floor(STANDBY_RETRY_MS * (0.8 + 0.4 * Math.random()))
    const timer = setTimeout(() => {
      this.standbyTimers.delete(functionId)
      if (this.functions.get(functionId) !== fn || !this.mayRegister(functionId, fn)) return
      this.replayRegistration(MessageType.RegisterFunction, fn.message)
      fn.sentEpoch = this.connectionEpoch
    }, delay)
    timer.unref?.()
    this.standbyTimers.set(functionId, timer)
  }

  private stopStandby(functionId: string): void {
    const timer = this.standbyTimers.get(functionId)
    if (!timer) return
    clearTimeout(timer)
    this.standbyTimers.delete(functionId)
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

  private async onInvokeFunction(
    invocation_id: string | undefined,
    function_id: string,
    input: unknown,
    metadata?: JsonValue,
    traceparent?: string,
    baggage?: string,
    observeFields: { caller?: CallerWire; observe_depth?: number; activation?: ActivationWire } = {},
  ): Promise<unknown> {
    const fn = this.functions.get(function_id)
    const getResponseTraceparent = () => injectTraceparent() ?? traceparent
    const getResponseBaggage = () => injectBaggage() ?? baggage

    if (fn?.handler) {
      const handler = fn.handler
      // The invocation context every handler sees (`currentInvocation()`), and
      // what the calls it makes carry as their caller.
      const ctx: ActiveInvocation = {
        function_id,
        invocation_id: invocation_id ?? crypto.randomUUID(),
        observe_depth: observeFields.observe_depth ?? 0,
      }
      if (observeFields.caller) ctx.caller = observeFields.caller
      if (observeFields.activation) ctx.activation = observeFields.activation

      // An observe.v2 host delivers its routed calls (v2 §8.2): the delivery
      // sees the input as it arrived (channel refs unresolved), and what iii's
      // delivery selects rides the answer (`observed`).
      const snapshot: readonly ObserverImpl[] = fn.observe.compiled
      const activations = fn.observe.activations
      const hook = this.functionHooks.get(function_id)?.deliver
      const delivering = this.engineFeatures.has(FEATURE_OBSERVE_V2) && (hook !== undefined || snapshot.length > 0)
      const startedAt = Date.now()
      const t0 = performance.now()
      let delivery: Delivery | undefined
      const settle = delivering
        ? (outcome: HandlerOutcome) => {
            delivery = new Delivery(fn.observe, {
              call: this.callOf(function_id, ctx.invocation_id, outcome, input, ctx.caller, metadata),
              thrown: outcome.ok ? undefined : outcome.error,
              snapshot,
              activations,
              started_at_ms: startedAt,
              duration_ms: performance.now() - t0,
              observe_depth: ctx.observe_depth,
              traceparent,
              baggage,
              routed: true,
            })
            return delivery.run(hook)
          }
        : undefined
      const resolvedInput = this.resolveChannelValue(input)
      const run = () => runInContext(ctx, () => handler(resolvedInput, metadata, traceparent, baggage, settle))

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
        this.sendMessage(
          MessageType.InvocationResult,
          buildInvocationResult({
            invocation_id,
            function_id,
            result,
            traceparent: getResponseTraceparent(),
            baggage: getResponseBaggage(),
            observed: delivery?.observed,
          }) as Omit<IIIMessage, 'message_type'>,
        )
      } catch (error) {
        this.sendMessage(
          MessageType.InvocationResult,
          buildInvocationResult({
            invocation_id,
            function_id,
            error: delivery ? delivery.errorBody(error) : handlerErrorBody(error),
            traceparent: getResponseTraceparent(),
            baggage: getResponseBaggage(),
            observed: delivery?.observed,
          }) as Omit<IIIMessage, 'message_type'>,
        )
      }
    } else {
      const errorCode = fn ? 'function_not_invokable' : 'function_not_found'
      const errorMessage = fn ? 'Function is HTTP-invoked and cannot be invoked locally' : 'Function not found'
      if (invocation_id) {
        this.sendMessage(
          MessageType.InvocationResult,
          buildInvocationResult({
            invocation_id,
            function_id,
            error: { code: errorCode, message: errorMessage },
            traceparent,
            baggage,
          }) as Omit<IIIMessage, 'message_type'>,
        )
      }
    }
  }

  /**
   * `WorkerRegistered` told this connection what the engine supports. The
   * functions that need `observe.v2` go out now, or stay unregistered without
   * it. With `observe.v1` the observers are replayed (after the functions) and
   * held reports flushed; without it `observe()` fails with `UNSUPPORTED` and
   * observer hooks are inactive.
   */
  private onEngineFeatures(features: string[]): void {
    this.engineFeatures = new Set(features)
    this.registeredOnConnection = true
    this.sendRegistrations()
    this.warnUnregisteredHookIds()
    const queued = this.observeQueue
    this.observeQueue = []
    if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      for (const message of queued) this.sendMessageRaw(JSON.stringify(message))
      for (const handle of this.observerHandles.values()) this.sendObserve(handle)
      this.reporter.flush()
      return
    }
    for (const handle of this.observerHandles.values()) handle.setState('error', UNSUPPORTED_REASON)
    for (const functionId of this.functions.keys()) {
      if (this.hasObserverHooks(functionId)) this.warnHooksInactive(functionId)
    }
    this.reporter.clear()
  }

  private onObserverState(id: string, wireState: string, reason?: ErrorBody): void {
    const handle = this.observerHandles.get(id)
    if (!handle) return
    const state = toObserverState(wireState)
    handle.setState(state, reason ? { code: reason.code, message: reason.message } : undefined)
    if (state === 'disposed') this.observerHandles.delete(id)
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
      const fn = this.functions.get(msg.id)
      if (fn) {
        // A standby function took over the id: its hooks run for every observer in the set.
        this.stopStandby(msg.id)
        fn.standingBy = false
        // Its reports held across a reconnect are flushed once the set is acked.
        fn.observe.onFunctionRegistered(msg, this.connectionEpoch)
      }
    } else if (msgType === MessageType.ObserverAdded) {
      const msg = decodeObserverAdded(message)
      this.functions.get(msg.function_id)?.observe.onObserverAdded(msg)
    } else if (msgType === MessageType.ObserverRemoved) {
      const msg = decodeObserverRemoved(message)
      this.functions.get(msg.function_id)?.observe.onObserverRemoved(msg)
    } else if (msgType === MessageType.ObserveResult) {
      const msg = decodeObserveResult(message)
      if (msg.warnings && msg.warnings.length > 0) {
        this.logWarn(`Observer ${msg.id}: ${msg.warnings.join('; ')}`)
      }
      this.onObserverState(msg.id, msg.state, msg.error)
    } else if (msgType === MessageType.ObserverStateChanged) {
      const msg = decodeObserverStateChanged(message)
      this.onObserverState(msg.id, msg.state, msg.reason)
    } else if (msgType === MessageType.WorkerRegistered) {
      const { worker_id, reattach_token, features } = decodeWorkerRegistered(message)
      this.workerId = worker_id
      this.reattachToken = reattach_token
      console.debug('[iii] Worker registered with ID:', worker_id)
      this.startMetricsReporting()
      this.onEngineFeatures(features ?? [])
    } else if (msgType === MessageType.RegistrationRejected) {
      this.onRegistrationRejected(decodeRegistrationRejected(message))
    }
  }
}

/**
 * Factory object that constructs routing actions for {@link IIIClient.call}.
 *
 * @example
 * ```typescript
 * import { CallAction } from 'iii-sdk'
 *
 * // Enqueue to a named queue
 * worker.call({
 *   function_id: 'process',
 *   payload: { data: 'hello' },
 *   action: CallAction.Enqueue({ queue: 'jobs' }),
 * })
 *
 * // Fire-and-forget
 * worker.call({
 *   function_id: 'notify',
 *   payload: {},
 *   action: CallAction.Void(),
 * })
 * ```
 */
export const CallAction = {
  /**
   * Routes the invocation through a named queue. The engine enqueues the job,
   * acknowledges the caller with `{ messageReceiptId }`, and processes it
   * asynchronously.
   *
   * Requires the `queue` worker in `worker-compose.yaml`.
   * Without it the call rejects with `enqueue_error` (no queue provider).
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

/** @deprecated Use {@link CallAction}, the same object. */
export const TriggerAction = CallAction

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
