import { createRequire } from 'node:module'
import * as os from 'node:os'
import { context, trace } from '@opentelemetry/api'
import { type Data, WebSocket } from 'ws'
import {
  abortInput,
  asStream,
  causeOf,
  detach,
  errorText,
  forwardableRef,
  handOnRefusal,
  handOver,
  InputStream,
  inputFromRef,
  isLocalSink,
  isTransferred,
  markTransferred,
  OutputStream,
  outputDone,
  outputEnded,
  outputFromRef,
  pumpStream,
  releaseInput,
  type StreamBinding,
  StreamScope,
  sinkFailureOf,
  streamInfoOf,
} from './call-streams'
import { streamInUseError } from './channels'
import { InvocationError, isErrorBody, ObserveRejection, RegistrationRejectedError, StreamAbortedError } from './errors'
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
  FEATURE_OBSERVE_CAPTURE,
  FEATURE_OBSERVE_V1,
  FEATURE_OBSERVE_V2,
  type IIIMessage,
  type InvocationResultMessage,
  type JsonValue,
  MessageType,
  type ObserverInfoWire,
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
  sameData,
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
  LEGACY_ON_DEPRECATION,
  legacyOn,
  stripCaller,
} from './observe-wire'
import { ObserverHandleImpl, toObserverState } from './observer-handle'
import { Reporter, type ReporterLimits } from './reporter'
import type { IStream } from './stream'
import {
  cloneKeepingStreams,
  containsStreams,
  containsStreamValues,
  holdsWriteSide,
  isObservedEnvelope,
  mapStreams,
  wireRef,
} from './stream-walk'
import type {
  FunctionRef,
  HandlerOutcome,
  IIIClient,
  Invocation,
  Pipe,
  PipeOptions,
  RegisterFunctionOptions,
  RemoteFunctionData,
  RemoteFunctionHandler,
} from './types'
import { detectProjectName, freezePlain, resolveHeaders } from './utils'

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

/** Why `call` refuses a call through an observer that names metadata (v2 §4.6, decision 53); the same in every SDK. */
const OBSERVER_CALL_METADATA =
  "a call through an observer takes no metadata; its target receives the observer's metadata"

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

/** The stream values of an outgoing value, replaced by refs, and the pumps that move their data. */
type OutboundStreams = {
  /** The value with refs in place of its streams. */
  value: unknown
  /** Starts the pumps; returns one promise per sink, settled when that sink finished. */
  start(): Promise<void>[]
  /** Stops the pumps (the call failed): the pipes' ends are aborted, the sinks with them. */
  cancel(error: unknown): void
  /** The sinks of this process the value carries, each fed through a pipe. */
  sinks: OutputStream[]
}

/** `STREAM_IN_USE`: one payload carries the same stream twice; a stream has one reader. */
function duplicateStream(): InvocationError {
  return streamInUseError('the same stream appears twice in one payload: a stream has one reader')
}

/** `STREAM_IN_USE`: the stream already went to another function by its ref. */
function alreadySent(): InvocationError {
  return streamInUseError('a stream in the payload was already sent to another function')
}

/** A stream in a call's metadata: it has no pipe there, and would reach the handler as `{}`. The Rust SDK's error. */
const STREAM_IN_METADATA = 'a stream cannot travel in metadata: put it in the payload'

/** `STREAM_NOT_ALLOWED`: a queued call cannot carry a stream (R9). */
function streamNotAllowed(function_id: string): InvocationError {
  return new InvocationError({
    code: 'STREAM_NOT_ALLOWED',
    message:
      'streams need a live call and cannot be queued: the job can run after the pipe expired, and a retry cannot read the stream again. Call the function without CallAction.Enqueue (CallAction.Void is allowed), or pass the data itself',
    function_id,
  })
}

/** Whether an observer of the snapshot captures the input or filters on it; computed once per snapshot. */
const inputReaders = new WeakMap<readonly ObserverImpl[], boolean>()
function readsInput(snapshot: readonly ObserverImpl[]): boolean {
  let reads = inputReaders.get(snapshot)
  if (reads === undefined) {
    reads = snapshot.some((obs) => obs.capture.includes('input') || obs.filter.roots.has('input'))
    inputReaders.set(snapshot, reads)
  }
  return reads
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
  // III_NAMESPACE carries the namespace for managed workers (set by iii compose
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
   * on the sockets of streams (InputStream, OutputStream). `Authorization: Bearer $III_TOKEN` is added unless you set
   * `Authorization` here.
   */
  headers?: Record<string, string>
  /** @internal */
  telemetry?: TelemetryOptions
  /** @internal Test knobs for the observe hooks and the reporter. */
  observe?: { hookTimeoutMs?: number; reporter?: Partial<ReporterLimits> }
}

/** The upgrade guide the removed 0.25 trigger APIs point to. */
const UPGRADE_GUIDE = 'https://iii.dev/docs/upgrading/from-0-25-x'

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
  // Observers this worker created, replayed (same id) on every connection
  // until the engine refuses one: the handle that holds each id (decision 56).
  private observerHandles = new Map<string, ObserverHandleImpl>()
  // Per id, the handles whose `observe` went out on this connection, oldest
  // first: the engine's answers name an id, and their order says which of
  // them each answer is about (decision 56). A disposed handle leaves once it
  // has its `observeresult`; the holder of the id is the last.
  private observesSent = new Map<string, ObserverHandleImpl[]>()
  // The engine's features (`WorkerRegistered.features`), kept across a
  // disconnect until the next `WorkerRegistered` replaces them.
  private engineFeatures: ReadonlySet<string> = new Set()
  /** Whether the legacy `on` deprecation (engines without `observe.capture`) was logged. */
  private legacyOnWarned = false
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
   * HTTP-invoked functions are deprecated (iii-http-functions) and will be
   * removed in an upcoming release: register a handler that makes the HTTP
   * call instead. Migration guide: https://iii.dev/docs/upgrading/migrate-from-http-functions
   *
   * A function that sets `args_format`, `private: true`, `observable: false`,
   * an `onObservationMatch` hook or an `onObservationDeliver` hook needs an engine
   * with `observe.v2`: it is registered once the connection's engine
   * advertised it, and against an older engine it stays unregistered (logged
   * once, `UNSUPPORTED`).
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

    // `localDispatch` is this SDK's alone; `private: false` and
    // `observable: true` are the defaults and are not sent.
    const { localDispatch, private: isPrivate, observable, ...wireOptions } = options ?? {}
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
    const fn: RemoteFunctionData = { message: fullMessage, observe }
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
      onObservationMatch: (hook: MatchHook<I, O> | null) => {
        this.onObservationMatch(functionId, hook)
        return ref
      },
      onObservationDeliver: (hook) => {
        this.onObservationDeliver(functionId, hook)
        return ref
      },
      // This registration's observers: one load, no lookup by id.
      observers: () => observe.observers(),
      isObserved: (call) => observe.isObserved(call as Partial<Call>),
      unregister: () => {
        // A stale ref (the id was unregistered and registered again) must not
        // touch the registration that replaced it: nothing to do.
        if (this.functions.get(functionId) !== fn) return
        this.sendMessage(MessageType.UnregisterFunction, { id: functionId }, true)
        this.functions.delete(functionId)
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

  /**
   * What a function needs `observe.v2` for (decision 18): `args_format`,
   * `private`, `observable: false`, a match hook or a deliver hook.
   */
  private needsObserveV2(functionId: string, fn: RemoteFunctionData): boolean {
    const { message } = fn
    const hooks = this.functionHooks.get(functionId)
    return (
      message.args_format !== undefined ||
      message.private === true ||
      message.observable === false ||
      hooks?.match !== undefined ||
      hooks?.deliver !== undefined
    )
  }

  /**
   * Whether the function's `registerfunction` may go out on the current
   * connection now. One that needs `observe.v2` waits for this connection's
   * `WorkerRegistered`, and stays unregistered (`holdUnsupported`) when the
   * engine does not advertise it (decision 41).
   */
  private mayRegister(functionId: string, fn: RemoteFunctionData): boolean {
    if (!this.isOpen()) return false
    if (!this.needsObserveV2(functionId, fn)) return true
    if (!this.registeredOnConnection) return false
    if (this.engineFeatures.has(FEATURE_OBSERVE_V2)) return true
    this.holdUnsupported(functionId, fn)
    return false
  }

  /**
   * A function that needs `observe.v2` on a connection whose engine does not
   * advertise it: the engine does not hold it here, so what it kept from an
   * earlier connection is released once per connection (local dispatch, its
   * observers through `onObserverRemoved`, held reports), and `UNSUPPORTED`
   * is logged once per function.
   */
  private holdUnsupported(functionId: string, fn: RemoteFunctionData): void {
    if (fn.observe.releasedEpoch !== this.connectionEpoch) fn.observe.onRegistrationRejected(this.connectionEpoch)
    this.warnUnsupported(functionId)
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
   * Decides the registration again once a match or deliver hook may have
   * changed whether the function needs `observe.v2` (set or removed, or set
   * before `WorkerRegistered`): against an engine without it, a registered
   * function is unregistered (and one held back is registered once it no
   * longer needs it).
   */
  private reconcileRegistration(functionId: string): void {
    const fn = this.functions.get(functionId)
    if (!fn || !this.isOpen() || !this.registeredOnConnection) return
    const sent = fn.sentEpoch === this.connectionEpoch
    const allowed = this.engineFeatures.has(FEATURE_OBSERVE_V2) || !this.needsObserveV2(functionId, fn)
    if (sent && !allowed) {
      this.sendMessage(MessageType.UnregisterFunction, { id: functionId }, true)
      fn.sentEpoch = undefined
      this.holdUnsupported(functionId, fn)
    } else if (!sent && allowed) {
      this.replayRegistration(MessageType.RegisterFunction, fn.message)
      fn.sentEpoch = this.connectionEpoch
    }
  }

  private warnUnsupported(functionId: string): void {
    if (this.unsupportedWarned.has(functionId)) return
    this.unsupportedWarned.add(functionId)
    this.logWarn(
      `UNSUPPORTED: function "${functionId}" needs observe.v2 (args_format, private, observable: false, onObservationMatch or onObservationDeliver), which this engine does not advertise; it stays unregistered until the worker connects to an engine with observe.v2.`,
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
    // A match or deliver hook decides whether the function needs observe.v2.
    if (kind === 'match' || kind === 'deliver') this.reconcileRegistration(functionId)
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
   * observers that want the call. `null` removes the hook: every observer
   * gets every call again.
   */
  onObservationMatch: IIIClient['onObservationMatch'] = (functionId: string, hook: MatchHook | null): IIIClient => {
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
  onObservationDeliver = <I = any, O = any, R = O>(
    functionId: string,
    hook: DeliverHook<I, O, R> | null,
  ): IIIClient => {
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
   * replayed with the same id on every reconnect; one without `max_fires` or
   * `expires_in_ms` that the engine disposed while the worker was away past the
   * grace period (`OBSERVER_DISCONNECTED`) is observed again. Errors (`INVALID_ARGS`,
   * `INVALID_FILTER`, `FORBIDDEN`, `NOT_OBSERVABLE`, the host's rejection,
   * `UNSUPPORTED` against an engine without observe) show up as state `error`
   * and reject `ready()`. An `error` answer to the `observe` is the engine's
   * refusal, the handle's final answer: the engine holds no observer for it,
   * it is not replayed, and its `dispose()` sends nothing. The handle holds
   * its id until it is disposed: an `observe` of an id a handle of this
   * client holds throws an {@link ObserveRejection} (`OBSERVER_ID_CONFLICT`)
   * and sends nothing.
   *
   * @param source - What is observed: a function id, or `function_id`,
   *   `args`, `filters`, and `capture` (what the payload carries, and with it
   *   which outcomes fire the target).
   * @param target - The function to invoke (registered first; until then the observer is `pending`).
   * @param options - Policy and metadata: `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * iii.observe('orders::create', 'search::reindex-order')
   *
   * iii.registerFunction('alerts::payment-failed', async ({ error, caller }: Observed) => notify(error, caller))
   * // `error` without `input` or `output`: fires on failures only.
   * const failures = iii.observe(
   *   { function_id: 'payments::charge', capture: ['error', 'caller'] },
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
    // Held by a handle of this client: refused here, nothing sent, the holder untouched.
    if (this.observerHandles.has(id)) {
      throw new ObserveRejection('OBSERVER_ID_CONFLICT', `observe: observer id already in use: ${id}`)
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
    // Not part of the 0.26 SDK API: refused, not dropped. Dropped, they would
    // leave the default capture to fire the target on other outcomes than the
    // ones asked for. (A 0.26 engine still translates them from 0.25 and release
    // candidate SDKs; this SDK never sends them.)
    const REMOVED_SOURCE_FIELDS = ['on', 'deliver'] as const
    for (const field of REMOVED_SOURCE_FIELDS) {
      if ((source as unknown as Record<string, unknown>)[field] !== undefined) {
        return refused({
          code: 'INVALID_ARGS',
          message: `observe: \`source.${field}\` was removed: \`source.capture\` decides which outcomes fire the target and what it receives`,
        })
      }
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

  /**
   * The `observe` frame for this connection's engine, on the first send and on
   * every replay. One without `observe.capture` (0.25, and rc engines that have
   * `observe.v2`) decides the outcomes that fire from `source.on` alone, so it
   * gets the `on` that `capture` implies; one with it never gets `on`.
   */
  private observeFrame(message: ObserverHandleImpl['message']): ObserverHandleImpl['message'] {
    if (this.engineFeatures.has(FEATURE_OBSERVE_CAPTURE)) return message
    const on = legacyOn(message.source.capture)
    if (on === undefined) return message
    this.warnLegacyOn()
    return { ...message, source: { ...message.source, on } as typeof message.source }
  }

  /** Logs, once per client, that the engine talks the deprecated legacy `on` (no `observe.capture`). */
  private warnLegacyOn(): void {
    if (this.legacyOnWarned) return
    this.legacyOnWarned = true
    this.logWarn(LEGACY_ON_DEPRECATION)
  }

  /** Observer views without `fires_on` come from an engine without `observe.capture`: they fire from the legacy `on`. */
  private noteLegacyViews(views: readonly ObserverInfoWire[]): void {
    if (!this.legacyOnWarned && views.some((view) => view.fires_on === undefined)) this.warnLegacyOn()
  }

  private sendObserve(handle: ObserverHandleImpl): void {
    this.sendMessage(MessageType.Observe, this.observeFrame(handle.message), true)
    handle.sent = true
    handle.answered = false
    const sent = this.observesSent.get(handle.id)
    if (sent) sent.push(handle)
    else this.observesSent.set(handle.id, [handle])
  }

  private disposeObserver(handle: ObserverHandleImpl): void {
    if (this.observerHandles.get(handle.id) === handle) this.observerHandles.delete(handle.id)
    // Answered, it leaves the list: what the engine says about it later
    // reaches no other handle (a newer one hears nothing before its answer).
    if (handle.answered) this.unlistObserve(handle)
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
   * Creates a pipe: an {@link InputStream} and an {@link OutputStream} joined
   * through the engine. What is written into `output` is read from `input`.
   * Put either end in a call's payload (or a handler's result) for the other
   * side to use: the end travels as its ref. This worker owns the pipe and
   * must stay connected until the other side attached.
   *
   * Most code does not need it: a source or a sink placed in a payload gets
   * its pipe from the SDK. Use it when the other end must exist before its
   * source does, for a two-way session (two pipes), or for a pipe handed to a
   * call made later.
   *
   * @param options - `bufferSize`: frames the engine holds for an end nobody reads yet.
   *
   * @example
   * ```typescript
   * const pipe = await iii.pipe()
   * const result = iii.call({ function_id: 'files::process', payload: { body: pipe.input } })
   * await pipeline(createReadStream('report.csv'), pipe.output.toWritable())
   * await result
   * ```
   */
  pipe = async (options?: PipeOptions): Promise<Pipe> => {
    const { writer, reader } = await this.createPipeRefs(options?.bufferSize)
    return { input: inputFromRef(reader, this.streamBinding()), output: outputFromRef(writer, this.streamBinding()) }
  }

  /** The engine and headers the streams of this worker connect with. */
  private streamBinding(): StreamBinding {
    return { wsBase: this.address, headers: this.options?.headers }
  }

  /** `engine::channels::create`, sent as it is: the two refs of a new pipe this worker owns. */
  private createPipeRefs(bufferSize?: number): Promise<{ writer: StreamChannelRef; reader: StreamChannelRef }> {
    const function_id = 'engine::channels::create'
    return this.invokeRemote(
      { function_id, payload: bufferSize === undefined ? {} : { buffer_size: bufferSize } },
      invocationNamespace(undefined, this.namespace, function_id),
    )
  }

  /**
   * Replaces the stream values of an outgoing `value` (a call's payload, a
   * handler's result) with refs (section 3 of the streams contract). An
   * InputStream never read and an OutputStream never used travel as their own
   * ref (R5). Every other source or sink gets a new pipe (created in parallel):
   * the reader ref goes in place of a source and the writer ref in place of a
   * sink, and `start()` runs the pumps. Refs already in `value` go as they are.
   * Rejects when a pipe cannot be created, before anything was sent.
   */
  private async prepareOutbound(value: unknown): Promise<OutboundStreams> {
    type Slot = { stream: InputStream | OutputStream; ref?: StreamChannelRef; end?: InputStream | OutputStream }
    const slots = new Map<unknown, Slot>()
    const slotOf = new Map<Slot, unknown>()
    for (const [slot, stream] of this.streamValues(value)) {
      const entry: Slot = { stream, ref: forwardableRef(stream) }
      slots.set(slot, entry)
      slotOf.set(entry, slot)
    }
    const forwarded = [...slots.values()].filter((entry) => entry.ref !== undefined)
    const piped = [...slots.values()].filter((entry) => entry.ref === undefined)
    const pipes = await Promise.all(piped.map(() => this.createPipeRefs()))
    const binding = this.streamBinding()
    piped.forEach((entry, i) => {
      const { reader, writer } = pipes[i]
      // Once its pipe exists, an InputStream passed as one counts as sent (the pump reads it through another),
      // and one a read began on is pumped from where that read is. A native source's wrapper is private already.
      if (entry.stream instanceof InputStream && entry.stream === slotOf.get(entry))
        entry.stream = handOver(entry.stream)
      // A sink gets the writer ref and is fed from the reader end; a source gets the reader ref and feeds the writer end.
      const sink = entry.stream instanceof OutputStream
      entry.ref = wireRef(sink ? writer : reader)
      entry.end = sink ? inputFromRef(reader, binding) : outputFromRef(writer, binding)
      detach(entry.stream)
    })
    for (const entry of forwarded) markTransferred(entry.stream)
    const replaced = mapStreams(value, (slot, kind) => (kind === 'ref' ? slot : slots.get(slot)?.ref))

    const running: Array<{ pump: Promise<void>; stop: (error: unknown) => void; sink: boolean }> = []
    return {
      value: replaced,
      sinks: piped
        .map((entry) => entry.stream)
        .filter((stream): stream is OutputStream => stream instanceof OutputStream),
      start: () => {
        for (const { stream, end } of piped) {
          if (stream instanceof OutputStream) {
            const input = end as InputStream
            const pump = pumpStream(input, stream)
            // Read once the call answered: a sink can fail before that.
            pump.catch(() => undefined)
            running.push({
              sink: true,
              pump,
              stop: (error) => {
                void stream.abort(errorText(error))
                void releaseInput(input)
              },
            })
          } else {
            const output = end as OutputStream
            const pump = pumpStream(stream, output)
            pump.catch((error) => {
              // A reader that left closes the writer end: not a failure of the source.
              if ((error as { code?: string })?.code === 'STREAM_CLOSED') return
              this.logWarn(`A stream source failed, and its reader got an abort: ${errorText(error)}`)
            })
            running.push({
              sink: false,
              pump,
              stop: (error) => {
                void output.abort(errorText(error))
                void releaseInput(stream)
              },
            })
          }
        }
        return running.filter((r) => r.sink).map((r) => r.pump)
      },
      cancel: (error) => {
        for (const { stop } of running) stop(error)
        // Not started: nothing reads the sources nor feeds the sinks; they are released here.
        if (running.length === 0) {
          for (const { stream } of piped) {
            if (stream instanceof OutputStream) void stream.abort(errorText(error))
            else void releaseInput(stream)
          }
        }
      },
    }
  }

  /**
   * The stream values of an outgoing `value` (not its refs), each as an
   * {@link InputStream} or {@link OutputStream}, keyed by the value it was.
   * Every refusal happens here, before any stream leaves its guard or is
   * adopted: the same stream twice, a stream already sent by its ref, an
   * InputStream read to its end (R6). A refused call has claimed nothing.
   */
  private streamValues(value: unknown): Map<unknown, InputStream | OutputStream> {
    const streams = new Map<unknown, InputStream | OutputStream>()
    mapStreams(value, (slot, kind) => {
      if (kind === 'ref') return slot
      if (streams.has(slot)) throw duplicateStream()
      const stream = asStream(slot)
      if (isTransferred(stream)) throw alreadySent()
      const refused = stream instanceof InputStream ? handOnRefusal(stream) : undefined
      if (refused) throw refused
      streams.set(slot, stream)
      return slot
    })
    return streams
  }

  /**
   * Turns the channel refs of a received `value` (a handler's input, a call's
   * result) into an {@link InputStream} (a read ref) or an {@link OutputStream}
   * (a write ref), bound to this worker, and gives them to `scope`. In an
   * `Observed` envelope (`observed`), the refs under its `input` and `output`
   * become a {@link StreamInfo} instead: a stream has one reader, the observed
   * function.
   */
  private inbound(value: unknown, scope?: StreamScope, observed = false): unknown {
    const binding = this.streamBinding()
    const live = (slot: unknown, kind: string) => {
      if (kind !== 'ref') return slot
      const ref = wireRef(slot as StreamChannelRef)
      const stream = ref.direction === 'read' ? inputFromRef(ref, binding) : outputFromRef(ref, binding)
      scope?.adopt(stream)
      return stream
    }
    if (!observed) return mapStreams(value, live)
    const envelope = value as Record<string, unknown>
    let out: Record<string, unknown> | undefined
    for (const [key, field] of Object.entries(envelope)) {
      const mapped =
        key === 'input' || key === 'output'
          ? mapStreams(field, (slot, kind) => (kind === 'ref' ? streamInfoOf(slot as StreamChannelRef) : slot))
          : mapStreams(field, live)
      if (mapped !== field) {
        out ??= { ...envelope }
        out[key] = mapped
      }
    }
    return out ?? envelope
  }

  /**
   * For a call this worker runs in-process: every stream value of `value`
   * becomes an {@link InputStream} or {@link OutputStream} (a Readable is
   * wrapped, an InputStream or
   * OutputStream is handed over as the routed path sends it, a ref is opened), with no
   * pipe. The streams join `scope` and are listed in `found`.
   */
  private localize(value: unknown, scope?: StreamScope, found?: Array<InputStream | OutputStream>): unknown {
    const binding = this.streamBinding()
    // The same refusals as the routed path (prepareOutbound), all of them before any stream is adopted.
    const values = this.streamValues(value)
    return mapStreams(value, (slot, kind) => {
      let stream: InputStream | OutputStream
      if (kind === 'ref') {
        const ref = wireRef(slot as StreamChannelRef)
        stream = ref.direction === 'read' ? inputFromRef(ref, binding) : outputFromRef(ref, binding)
      } else {
        const given = values.get(slot) as InputStream | OutputStream
        // An InputStream or OutputStream passed as one goes as the routed path sends it, and counts as sent.
        // A native source or sink is wrapped already, in a stream nobody else holds.
        stream = given === slot ? this.handOverLocally(given) : given
      }
      if (scope) scope.adopt(stream)
      else detach(stream)
      found?.push(stream)
      return stream
    })
  }

  /**
   * What the function a call runs in-process gets for `s`, as the routed path
   * would send it (R5): a stream with a ref, never read or used, is opened
   * again from its ref; another InputStream is handed over ({@link handOver}).
   * Either way `s` counts as sent, and sending or reading it again fails with
   * `STREAM_IN_USE`. Any other OutputStream (a sink of this worker, one in use)
   * goes as it is.
   */
  private handOverLocally(s: InputStream | OutputStream): InputStream | OutputStream {
    const ref = forwardableRef(s)
    if (ref) {
      const binding = this.streamBinding()
      const opened = s instanceof InputStream ? inputFromRef(ref, binding) : outputFromRef(ref, binding)
      markTransferred(s)
      return opened
    }
    return s instanceof InputStream ? handOver(s) : s
  }

  /**
   * The routed path with streams: the payload's stream values become refs
   * (pipes created first), the call goes out, the sources' pumps start, and the
   * result's refs become streams. The call resolves once its sinks finished
   * (R4): an aborted one rejects it with `STREAM_ABORTED`. A failed call stops
   * the pumps and aborts its sinks; so does the call's timeout when it expires
   * before the sinks finished (it rejects with `TIMEOUT`).
   */
  private async invokeRemoteStreams<TOutput>(
    request: { function_id: string; payload: unknown; action?: CallActionType; timeoutMs?: number; metadata?: unknown },
    namespace: string | undefined,
    activation?: ActivationWire,
  ): Promise<TOutput> {
    if (!containsStreams(request.payload)) {
      return this.inbound(await this.invokeRemote(request, namespace, activation)) as TOutput
    }
    const outbound = await this.prepareOutbound(request.payload)
    // The call's timeout runs from here, as the invocation's own (invokeRemote starts it now).
    const timeoutMs = request.timeoutMs ?? this.invocationTimeoutMs
    const startedAt = performance.now()
    const sent = this.invokeRemote<unknown>({ ...request, payload: outbound.value }, namespace, activation)
    const sinks = outbound.start()
    if (request.action?.type === 'void') {
      for (const sink of sinks)
        sink.catch((error) => this.logError(`Stream sink of ${request.function_id} failed`, error))
    }
    let result: unknown
    try {
      result = await sent
    } catch (error) {
      outbound.cancel(error)
      // The caller's own sink failed: that is why the call failed (the function's writes failed after it).
      throw this.failedSink(outbound.sinks, request.function_id) ?? error
    }
    // Fire-and-forget: the pumps run on, nobody waits for the sinks.
    if (request.action?.type === 'void') return result as TOutput
    const received = this.inbound(result)
    // A result that hands the caller a write side (its own sink passed back, a pipe into it)
    // can be what the sinks wait for: waiting here would never end. They run on.
    if (holdsWriteSide(result)) {
      for (const sink of sinks)
        sink.catch((error) => this.logError(`Stream sink of ${request.function_id} failed`, error))
      return received as TOutput
    }
    // The call's timeout bounds the wait for the sinks too: when it expires first, the pumps stop
    // (the sinks are aborted, their reader ends cancelled) and the call rejects with TIMEOUT.
    try {
      await this.awaitSinks(sinks, request.function_id, timeoutMs, startedAt, (error) => {
        outbound.cancel(error)
        this.discardStreams(received)
      })
    } catch (error) {
      throw this.failedSink(outbound.sinks, request.function_id) ?? error
    }
    return received as TOutput
  }

  /**
   * Waits for a call's sinks (R4) until the call's deadline, `timeoutMs` after
   * `startedAt` (a `performance.now()`). Rejects with the first sink failure,
   * or, when the deadline passes first, runs `expired` and rejects with the
   * call's `TIMEOUT` error.
   */
  private async awaitSinks(
    sinks: Array<Promise<void>>,
    function_id: string,
    timeoutMs: number,
    startedAt: number,
    expired: (error: InvocationError) => void,
  ): Promise<void> {
    if (sinks.length === 0) return
    let timer: NodeJS.Timeout | undefined
    const deadline = new Promise<'timeout'>((resolve) => {
      timer = setTimeout(() => resolve('timeout'), Math.max(0, startedAt + timeoutMs - performance.now()))
    })
    try {
      const outcome = await Promise.race([Promise.allSettled(sinks), deadline])
      if (outcome === 'timeout') {
        const error = this.timeoutError(function_id, timeoutMs)
        expired(error)
        throw error
      }
      for (const settled of outcome) {
        if (settled.status === 'rejected') throw this.sinkFailure(settled.reason, function_id)
      }
    } finally {
      clearTimeout(timer)
    }
  }

  /** The error a call rejects with when its timeout (`timeoutMs`, or the client default) expired. */
  private timeoutError(function_id: string, timeoutMs: number): InvocationError {
    return new InvocationError({ code: 'TIMEOUT', message: `invocation timed out after ${timeoutMs}ms`, function_id })
  }

  /** The error a call rejects with when one of its sinks failed: an abort is `STREAM_ABORTED`. */
  private sinkFailure(error: unknown, function_id: string): Error {
    if (error instanceof StreamAbortedError) return new StreamAbortedError(error.reason, { function_id, cause: error })
    return error instanceof Error ? error : new Error(errorText(error))
  }

  /**
   * The error a call rejects with when a sink of this process it passed
   * failed: `STREAM_ABORTED`, reason `the sink failed: …`, the sink's own
   * error as its `cause`. It wins over the call's other errors: the handler's
   * writes failed after it.
   */
  private failedSink(sinks: OutputStream[], function_id: string): StreamAbortedError | undefined {
    for (const sink of sinks) {
      const failure = sinkFailureOf(sink)
      if (failure) return new StreamAbortedError(failure.reason, { function_id, cause: causeOf(failure) })
    }
    return undefined
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
   * payload goes as given. It takes no metadata: the target's metadata
   * argument is the observer's registration metadata, and a request that
   * names metadata is refused before anything is sent.
   *
   * @param request - The call request.
   * @param request.function_id - ID of the function to invoke.
   * @param request.observer - An observer of a function this worker hosts: its target is invoked.
   * @param request.payload - Payload to pass to the function.
   * @param request.action - Routing action. Omit for synchronous request/response.
   * @param request.timeoutMs - Override the default invocation timeout. It also bounds the wait for the call's sinks.
   * @param request.metadata - Optional per-invocation metadata (any JSON value)
   *   surfaced to the target handler as its second argument. `null` is the
   *   same as no metadata: the handler receives `undefined`.
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
   * worker.onObservationDeliver('http::on-request', async (call) => {
   *   const route = routes.match(call.input)
   *   return route ? worker.call({ observer: route.obs, payload: call.input }) : { status_code: 404 }
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  call = async <TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput> => {
    // A queued job can run after its pipes expired, and a retry cannot read a stream again (R9).
    if (request.action?.type === 'enqueue' && containsStreams(request.payload)) {
      throw streamNotAllowed(request.function_id ?? request.observer?.target ?? '')
    }
    // Refused before anything is sent or claimed: the payload's streams stay the caller's.
    const callMetadata = (request as { metadata?: unknown }).metadata
    if (callMetadata != null && containsStreamValues(callMetadata)) throw new Error(STREAM_IN_METADATA)
    if (request.observer !== undefined) {
      const { observer, payload, action, timeoutMs } = request
      // Its target's metadata is the observer's: refused before anything is
      // sent, so no metadata is lost on the way (decision 53). `null` is none.
      if ((request as { metadata?: unknown }).metadata != null) throw new Error(OBSERVER_CALL_METADATA)
      // The engine routes to the observer's stored target and is the one that
      // checks it (decision 13); no namespace travels.
      return this.invokeRemoteStreams<TOutput>(
        { function_id: observer.target, payload, action, timeoutMs },
        undefined,
        { observer_id: observer.id },
      )
    }
    // `metadata: null` is no metadata, local or routed (decision 54).
    const req = request.metadata === null ? { ...request, metadata: undefined } : request
    const { function_id, action } = req
    // Engine-owned builtins stay in `default`. Other implicit calls stay in
    // this worker's namespace. An explicit request namespace always wins.
    const namespace = invocationNamespace(req.namespace, this.namespace, function_id)

    // Local dispatch (sdk.md §3): a function this worker hosts, confirmed by
    // the engine for in-process calls, in the namespace it registered in.
    // `enqueue` expects a queue receipt, so it always goes to the engine, and
    // so does a call already MAX_LOCAL_DEPTH local calls deep.
    if (req.dispatch !== 'remote' && action?.type !== 'enqueue') {
      const fn = this.functions.get(function_id)
      const observe = fn?.observe
      if (
        fn?.handler &&
        observe?.localDispatch &&
        (namespace === undefined || namespace === observe.namespace) &&
        (currentContext()?.localDepth ?? 0) < MAX_LOCAL_DEPTH
      ) {
        return this.invokeLocal<TOutput>(function_id, fn, req)
      }
    }
    return this.invokeRemoteStreams<TOutput>(req, namespace)
  }

  /**
   * Invokes a function using a request object.
   *
   * @deprecated Use {@link Sdk.call}, the same function.
   */
  trigger = this.call

  /** Removed in 0.26.0: throws an error that points to {@link Sdk.observe}. */
  registerTrigger = (): never => {
    throw new Error(
      `registerTrigger was removed in iii 0.26.0: bind functions with iii.observe(source, target). registerTrigger({ type: 'cron', function_id, config }) becomes iii.observe({ function_id: 'cron::on-tick', args: config }, function_id). Every trigger type's function is in the migration guide: ${UPGRADE_GUIDE}`,
    )
  }

  /** Removed in 0.26.0: throws an error that points to {@link Sdk.registerFunction} and the observer hooks. */
  registerTriggerType = (): never => {
    throw new Error(
      `registerTriggerType was removed in iii 0.26.0: a trigger type is now a function that consumers observe. Register it with registerFunction('<worker>::on-<event>', (event) => event, { args_format, private: true }), move the TriggerHandler to onObserverAdded and onObserverRemoved, and call the function once per event. Migration guide: ${UPGRADE_GUIDE}#move-a-trigger-type-to-an-observable-function`,
    )
  }

  /** Removed in 0.26.0: throws an error that points to `FunctionRef.unregister`. */
  unregisterTriggerType = (): never => {
    throw new Error(
      `unregisterTriggerType was removed in iii 0.26.0: a trigger type is now a function. Unregister it with the FunctionRef that registerFunction returns: functionRef.unregister(). Migration guide: ${UPGRADE_GUIDE}#move-a-trigger-type-to-an-observable-function`,
    )
  }

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
    // The engine refused this worker for good: nothing reaches it.
    if (this.fatalError) return Promise.reject(this.fatalError)
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
          reject(this.timeoutError(function_id, effectiveTimeout))
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
    // The delivery's copy of the input is taken only when something reads it:
    // a deliver or match hook (worker code), or an observer that captures the
    // input or filters on it.
    const copiesInput =
      delivering &&
      (hook !== undefined || this.functionHooks.get(function_id)?.match !== undefined || readsInput(snapshot))
    const byReference = fn.message.private === true
    // Streams in the payload reach the handler as streams, in-process (no pipe):
    // they join this invocation's guard, and the sinks among them hold the call
    // until they finished (R4). Copies keep the stream objects.
    const streamed = containsStreams(request.payload)
    const scope = streamed ? new StreamScope() : undefined
    const passed: Array<InputStream | OutputStream> = []
    const copy = streamed ? (value: unknown) => cloneKeepingStreams(value, cloneValue) : cloneValue
    let input: unknown
    let metadata: unknown
    let callInput: unknown
    let callMetadata: unknown
    try {
      const payload = streamed ? this.localize(request.payload, scope, passed) : request.payload
      if (byReference) {
        // Nobody else sees either value: neither is copied, and both are read-only.
        if (this.freezePrivate) {
          freezePlain(payload)
          freezePlain(request.metadata)
        }
        callInput = payload
        callMetadata = request.metadata
        input = payload
        metadata = request.metadata
      } else {
        // What the delivery sees is the payload and metadata as the caller passed
        // them when the call started, not what the handler or the caller made of
        // them since. Not copied when nothing is delivered.
        if (copiesInput) {
          callInput = copy(payload)
          callMetadata = request.metadata === undefined ? undefined : cloneValue(request.metadata)
        }
        input = copy(payload)
        metadata = request.metadata === undefined ? undefined : cloneValue(request.metadata)
      }
    } catch (error) {
      return Promise.reject(error)
    }
    const sinks = passed.filter((s): s is OutputStream => s instanceof OutputStream && isLocalSink(s))
    // The sources the routed path would pipe (not a ref passed on unread): a call that ends early aborts them,
    // so the handler's read fails with STREAM_ABORTED as it does across a pipe.
    const sources = passed.filter((s): s is InputStream => s instanceof InputStream && forwardableRef(s) === undefined)
    /** The call failed: sinks it passed that are still open are aborted with its error. */
    const abortSinks = (error: unknown) => {
      for (const sink of sinks) if (!outputEnded(sink)) void sink.abort(errorText(error))
    }

    const startedAt = Date.now()
    const t0 = performance.now()
    let delivery: Delivery | undefined
    const settle = (settled: HandlerOutcome): unknown => {
      const duration = performance.now() - t0
      this.reporter.recordLocal(function_id, settled.ok, duration)
      // Streams in the result go to the caller as streams: they leave the handler's guard.
      const handled: HandlerOutcome =
        settled.ok && containsStreams(settled.value) ? { ok: true, value: this.localize(settled.value) } : settled
      if (!delivering) {
        if (handled.ok) return handled.value
        throw handled.error
      }
      const outcome = this.deliveredOutcome(handled, input, callInput, byReference)
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
      (output) => {
        // A plain call: an untouched OutputStream is closed on success; in-process,
        // a source of this process nobody read is released too.
        scope?.settle({ ok: true, value: output }, { local: true })
        return output
      },
      (error) => {
        scope?.settle({ ok: false, error }, { local: true })
        throw new InvocationError({ ...(delivery ? delivery.errorBody(error) : handlerErrorBody(error)), function_id })
      },
    )

    if (request.action?.type === 'void') {
      done.catch((error) => {
        abortSinks(error)
        this.logError(`Error invoking function ${function_id}`, error)
      })
      return Promise.resolve(undefined as TOutput)
    }

    const effectiveTimeout = request.timeoutMs ?? this.invocationTimeoutMs
    return new Promise<TOutput>((resolve, reject) => {
      // The streams of a result the caller will not get (the call timed out): released when it expired.
      let unclaimed: unknown
      let timedOut = false
      // Runs until the handler returned and the sinks finished (R4): the call's timeout bounds both.
      const timeout = setTimeout(() => {
        timedOut = true
        const error = this.timeoutError(function_id, effectiveTimeout)
        abortSinks(error)
        for (const source of sources) void abortInput(source, errorText(error))
        this.discardStreams(unclaimed)
        reject(this.failedSink(sinks, function_id) ?? error)
      }, effectiveTimeout)
      done.then(
        async (output) => {
          try {
            const withStreams = containsStreams(output)
            const result = withStreams ? this.localize(output) : output
            if (timedOut) {
              this.discardStreams(result)
              return
            }
            unclaimed = result
            // As on the routed path: a result that hands the caller a write side ends the wait for the sinks.
            const waited = withStreams && holdsWriteSide(result) ? [] : sinks
            const outcomes = await Promise.allSettled(waited.map(outputDone))
            // The caller's own sink failed: that is why the call failed, whatever the handler did.
            const failed = this.failedSink(sinks, function_id)
            if (failed) throw failed
            for (const settled of outcomes) {
              if (settled.status === 'rejected') throw this.sinkFailure(settled.reason, function_id)
            }
            clearTimeout(timeout)
            if (timedOut) return
            if (byReference) resolve((this.freezePrivate ? freezePlain(result) : result) as TOutput)
            else if (withStreams) resolve(cloneKeepingStreams(result, cloneValue) as TOutput)
            else resolve(cloneValue(result) as TOutput)
          } catch (error) {
            clearTimeout(timeout)
            reject(error)
          }
        },
        (error) => {
          clearTimeout(timeout)
          abortSinks(error)
          reject(this.failedSink(sinks, function_id) ?? error)
        },
      )
    })
  }

  /**
   * The outcome a local call is delivered with. A private function's result
   * is frozen in development, before the match and deliver hooks and the
   * report read it, so a write from any of them throws. A handler that
   * returns its own input unchanged is delivered with the input the call
   * started with as its output, so the report carries the value once
   * (`output_is_input`, decision 4).
   */
  private deliveredOutcome(
    outcome: HandlerOutcome,
    input: unknown,
    callInput: unknown,
    byReference: boolean,
  ): HandlerOutcome {
    if (!outcome.ok) return outcome
    if (byReference) {
      if (this.freezePrivate) freezePlain(outcome.value)
      return outcome
    }
    const ownInput = outcome.value === input && input !== null && typeof input === 'object'
    return ownInput && sameData(callInput, input) ? { ok: true, value: callInput } : outcome
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
        // Opts this connection into the observe messages.
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
    this.observesSent.clear()

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
    this.observesSent.clear()

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

  /**
   * The `payload_too_large` error that replaces an answer too large to send
   * is not the handler's outcome, so it activates no observer: to an engine
   * with `observe.v2` it says `delivered` when the deliver hook delivered the
   * call (`answer.delivered`), and `matched: []` otherwise, whatever the
   * function's hooks. An engine without `observe.v2` gets the bare error.
   */
  private oversizedAnswer(answer: Record<string, unknown>, fallback: Record<string, unknown>): Record<string, unknown> {
    if (!this.engineFeatures.has(FEATURE_OBSERVE_V2)) return fallback
    if (answer.delivered === true) return { ...fallback, delivered: true }
    return { ...fallback, matched: [] }
  }

  // Classify the complete serialized envelope against the inclusive message limit.
  private prepareJson(message: Record<string, unknown>): string {
    const data = JSON.stringify(message)
    const size = Buffer.byteLength(data, 'utf8')
    if (size <= MAX_JSON_MESSAGE_BYTES) return data
    const detail = `Serialized JSON envelope is ${size} bytes; limit is ${MAX_JSON_MESSAGE_BYTES} bytes. Use streams for larger payloads: https://iii.dev/docs/creating-workers/streaming#payload-size`
    const error = new InvocationError({ code: 'payload_too_large', message: detail })
    if (message.type === MessageType.InvocationResult) {
      // The wire body carries the bare message: the caller's InvocationError adds the `code: ` prefix.
      const fallback = JSON.stringify(
        this.oversizedAnswer(message, {
          type: MessageType.InvocationResult,
          invocation_id: message.invocation_id,
          function_id: message.function_id,
          error: { code: error.code, message: detail },
        }),
      )
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
   *   other functions. Non-fatal -- log a warning and continue.
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

    // Tear down the socket without starting the reconnect path.
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

  /** Closes the sources of a result nobody receives (a fire-and-forget call, a result that failed to go out). */
  private discardStreams(value: unknown): void {
    if (!containsStreams(value)) return
    mapStreams(value, (slot, kind) => {
      if (kind === 'source' || kind === 'input') void releaseInput(asStream(slot) as InputStream)
      return slot
    })
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

      // The engine evaluates the capture and filters of the calls it routes
      // (decision 55). An observe.v2 host runs the function's match and
      // deliver hooks for them, which see the input as it arrived (channel
      // refs unresolved), and its answer says how the call is delivered:
      // `delivered`, `matched`, or neither for a function with no hook.
      const snapshot: readonly ObserverImpl[] = fn.observe.compiled
      const activations = fn.observe.activations
      const hook = this.functionHooks.get(function_id)?.deliver
      const startedAt = Date.now()
      const t0 = performance.now()
      let delivery: Delivery | undefined
      // The answer's `matched` when the call needs no delivery.
      let matched: readonly string[] | undefined
      const settle = this.engineFeatures.has(FEATURE_OBSERVE_V2)
        ? (outcome: HandlerOutcome) => {
            if (hook === undefined) {
              // No delivery runs without a deliver hook when the function has
              // no match hook (the answer carries neither field: the engine
              // evaluates every active observer), or when no observer was
              // active as the call started (`matched: []`: the match hook has
              // none to match and does not run, as a local call with an empty
              // snapshot skips delivery).
              const narrowed = fn.observe.hasMatchHook()
              if (!narrowed || snapshot.length === 0) {
                if (narrowed) matched = []
                if (outcome.ok) return outcome.value
                throw outcome.error
              }
            }
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
      // The refs of the input become streams this invocation's guard tracks; in
      // an `Observed` envelope, those under `input` and `output` become StreamInfo.
      const scope = new StreamScope()
      // An observer activation (an HTTP route target) keeps an untouched OutputStream open for a non-null result.
      const guard = { activation: observeFields.activation !== undefined }
      const resolvedInput = this.inbound(input, scope, isObservedEnvelope(input, observeFields.activation))
      const run = () => runInContext(ctx, () => handler(resolvedInput, metadata, traceparent, baggage, settle))

      // Fire-and-forget (`void`, no invocation id): never answered. Activations
      // carry an id and are answered like any call.
      if (!invocation_id) {
        try {
          const result = await run()
          scope.settle({ ok: true, value: result }, guard)
          this.discardStreams(result)
        } catch (error) {
          scope.settle({ ok: false, error }, guard)
          this.logError(`Error invoking function ${function_id}`, error)
        }
        return
      }

      let outcome: HandlerOutcome
      try {
        outcome = { ok: true, value: await run() }
      } catch (error) {
        outcome = { ok: false, error }
      }
      // Streams in the result travel as refs: their pipes first, their pumps once the answer is out.
      let outbound: OutboundStreams | undefined
      if (outcome.ok && containsStreams(outcome.value)) {
        try {
          outbound = await this.prepareOutbound(outcome.value)
          outcome = { ok: true, value: outbound.value }
        } catch (error) {
          this.discardStreams(outcome.value)
          outcome = { ok: false, error }
        }
      }
      // The guard starts its closes and cancels before the answer goes out (R2, R3).
      scope.settle(outcome, guard)
      const answer = (o: HandlerOutcome) =>
        this.sendMessage(
          MessageType.InvocationResult,
          buildInvocationResult({
            invocation_id,
            function_id,
            ...(o.ok
              ? { result: o.value }
              : { error: delivery ? delivery.errorBody(o.error) : handlerErrorBody(o.error) }),
            traceparent: getResponseTraceparent(),
            baggage: getResponseBaggage(),
            matched: delivery ? delivery.matched : matched,
            delivered: delivery?.delivered,
          }) as Omit<IIIMessage, 'message_type'>,
        )
      try {
        answer(outcome)
      } catch (error) {
        // The result could not go out (not serializable): its streams are dropped, and the caller gets the error.
        outbound?.cancel(error)
        if (!outcome.ok) throw error
        answer({ ok: false, error })
        return
      }
      for (const sink of outbound?.start() ?? []) {
        sink.catch((error) => this.logError(`Stream sink returned by ${function_id} failed`, error))
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
   * it; without it, one sent earlier on this connection that has gained a
   * match or deliver hook since is unregistered. With `observe.v1` the observers the
   * engine has not refused are replayed (after the functions) and held reports flushed; without it
   * `observe()` fails with `UNSUPPORTED` and observer hooks are inactive.
   */
  private onEngineFeatures(features: string[]): void {
    this.engineFeatures = new Set(features)
    this.registeredOnConnection = true
    this.sendRegistrations()
    for (const [functionId, fn] of this.functions) {
      if (fn.sentEpoch === this.connectionEpoch) this.reconcileRegistration(functionId)
    }
    this.warnUnregisteredHookIds()
    const queued = this.observeQueue
    this.observeQueue = []
    if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      for (const message of queued) this.sendMessageRaw(JSON.stringify(message))
      // A refused handle is not sent again: its refusal is its final answer.
      for (const handle of this.observerHandles.values()) {
        if (!handle.refused) this.sendObserve(handle)
      }
      this.reporter.flush()
      return
    }
    for (const handle of this.observerHandles.values()) {
      if (!handle.refused) handle.setState('error', UNSUPPORTED_REASON)
    }
    for (const functionId of this.functions.keys()) {
      if (this.hasObserverHooks(functionId)) this.warnHooksInactive(functionId)
    }
    this.reporter.clear()
  }

  /**
   * An `observeresult`: the answer to the oldest `observe` of the id sent on
   * this connection that has none yet. The engine answers every `observe`
   * once, in the order it read them (decision 56).
   */
  private onObserveResult(id: string, wireState: string, reason?: ErrorBody): void {
    const handle = this.observesSent.get(id)?.find((h) => !h.answered)
    if (!handle) return
    handle.answered = true
    if (toObserverState(wireState) === 'error') {
      // A refusal, the handle's final answer: the engine holds no observer
      // for it (it stores nothing for a refused `observe`, and disposes an
      // observer in `error` as it answers its replay), so dispose() sends
      // no `unobserve`.
      handle.sent = false
      handle.refused = true
      this.unlistObserve(handle)
      handle.setState('error', reason ? { code: reason.code, message: reason.message } : undefined)
      return
    }
    // Disposed before its answer: its `unobserve` follows, and nothing
    // the engine says about it later reaches another handle.
    if (handle.state === 'disposed') this.unlistObserve(handle)
    else this.applyObserverState(handle, wireState, reason)
  }

  /**
   * An `observerstatechanged`: about the newest observer of the id whose
   * `observeresult` arrived on this connection. One that arrives before any
   * did is dropped: a reattached connection hears its observers resume
   * before its replayed `observe`s are answered, and each answer carries the
   * current state (decision 56).
   */
  private onObserverStateChanged(id: string, wireState: string, reason?: ErrorBody): void {
    const sent = this.observesSent.get(id)
    if (!sent) return
    for (let at = sent.length - 1; at >= 0; at--) {
      if (sent[at].answered) {
        this.applyObserverState(sent[at], wireState, reason)
        return
      }
    }
  }

  /** A state from the engine for `handle`; `disposed` frees its id, unless it is observed again. */
  private applyObserverState(handle: ObserverHandleImpl, wireState: string, reason?: ErrorBody): void {
    const state = toObserverState(wireState)
    if (state === 'disposed' && reason?.code === 'OBSERVER_DISCONNECTED' && this.observeAgain(handle)) return
    handle.setState(state, reason ? { code: reason.code, message: reason.message } : undefined)
    if (state !== 'disposed') return
    this.unlistObserve(handle)
    if (this.observerHandles.get(handle.id) === handle) this.observerHandles.delete(handle.id)
  }

  /**
   * The engine disposed `handle`'s observer while this worker was away past
   * the grace period (`OBSERVER_DISCONNECTED`): the id is free on this
   * connection, so the handle observes it again as a new observer and stays
   * held (E17). Not one with `max_fires` or `expires_in_ms` (a new observer
   * would restart its counters, decision 56), one refused or in `error`, or
   * one already observed again on this connection. One its worker disposed
   * never gets here: what the engine says about it later is dropped.
   */
  private observeAgain(handle: ObserverHandleImpl): boolean {
    const { max_fires, expires_in_ms } = handle.message
    if (max_fires != null || expires_in_ms != null || handle.state === 'error') return false
    if (handle.observedAgainOn === this.connectionEpoch) return false
    this.logWarn(
      `Observer ${handle.id}: the engine disposed it while this worker was away past the grace period; observing it again`,
    )
    handle.observedAgainOn = this.connectionEpoch
    // Answered: out of the list, then in again as the newest observe of the id.
    this.unlistObserve(handle)
    handle.setState('pending')
    this.sendObserve(handle)
    return true
  }

  private unlistObserve(handle: ObserverHandleImpl): void {
    const sent = this.observesSent.get(handle.id)
    const at = sent ? sent.indexOf(handle) : -1
    if (!sent || at < 0) return
    sent.splice(at, 1)
    if (sent.length === 0) this.observesSent.delete(handle.id)
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
      this.noteLegacyViews(msg.observers)
      const fn = this.functions.get(msg.id)
      // Only a registration sent on this connection is confirmed: not one
      // unregistered since (a match or deliver hook against an engine without observe.v2).
      if (fn && fn.sentEpoch === this.connectionEpoch) {
        // Its reports held across a reconnect are flushed once the set is acked.
        fn.observe.onFunctionRegistered(msg, this.connectionEpoch)
      }
    } else if (msgType === MessageType.ObserverAdded) {
      const msg = decodeObserverAdded(message)
      this.noteLegacyViews([msg.observer])
      const fn = this.functions.get(msg.function_id)
      // An observer of a registration this connection withdrew (or never
      // sent) is ignored, as its functionregistered is: no hook, no ack.
      if (fn && fn.sentEpoch === this.connectionEpoch) fn.observe.onObserverAdded(msg)
    } else if (msgType === MessageType.ObserverRemoved) {
      const msg = decodeObserverRemoved(message)
      this.functions.get(msg.function_id)?.observe.onObserverRemoved(msg)
    } else if (msgType === MessageType.ObserveResult) {
      const msg = decodeObserveResult(message)
      if (msg.warnings && msg.warnings.length > 0) {
        this.logWarn(`Observer ${msg.id}: ${msg.warnings.join('; ')}`)
      }
      this.onObserveResult(msg.id, msg.state, msg.error)
    } else if (msgType === MessageType.ObserverStateChanged) {
      const msg = decodeObserverStateChanged(message)
      this.onObserverStateChanged(msg.id, msg.state, msg.reason)
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
