import { ChannelReader, ChannelWriter } from './channels'
import {
  handlerErrorBody,
  InvocationError,
  InvocationTimeoutError,
  ObserveRejection,
  RegistrationRejectedError,
} from './errors'
import { compileFilter, FilterError } from './filter'
import {
  DEFAULT_BRIDGE_RECONNECTION_CONFIG,
  DEFAULT_INVOCATION_TIMEOUT_MS,
  EngineFunctions,
  type IIIConnectionState,
  type IIIReconnectionConfig,
} from './iii-constants'
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
  MessageType,
  type RegisterFunctionMessage,
  SDK_FEATURES,
  type StreamChannelRef,
} from './iii-types'
import {
  type ActiveInvocation,
  callContext,
  callDepth,
  callLocalDepth,
  contextFromView,
  currentContext,
  enterInvocation,
  MAX_LOCAL_DEPTH,
  MAX_OFFLINE_LOCAL_DEPTH,
  MAX_RUNNING_INVOCATION_MS,
  publicContext,
  runInContext,
  trackDelivering,
  trackRunning,
} from './invocation-context'
import {
  Delivery,
  type FunctionHooks,
  FunctionObservers,
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
  stripCaller,
} from './observe-wire'
import { ObserverHandleImpl, toObserverState } from './observer-handle'
import { Reporter, type ReporterLimits } from './reporter'
import type { IStream } from './stream'
import type {
  FunctionRef,
  Invocation,
  ISdk,
  RegisterFunctionOptions,
  RemoteFunctionData,
  RemoteFunctionHandler,
} from './types'
import { isChannelRef, randomUUID, unrefTimer } from './utils'

/** @internal */
export type TelemetryOptions = {
  language?: string
  project_name?: string
  framework?: string
}

// Browser WebSocket APIs expose no control over framing, so outbound JSON is limited to the 16 MiB single-frame size.
const MAX_JSON_FRAME_BYTES = 16 * 1024 * 1024
const JSON_ENCODER = new TextEncoder()
const PAYLOAD_SIZE_DOCS = 'https://iii.dev/docs/creating-workers/channels#payload-size'

function payloadTooLargeMessage(size: number): string {
  return `Serialized JSON envelope is ${size} bytes; limit is ${MAX_JSON_FRAME_BYTES} bytes. Use channels for larger payloads: ${PAYLOAD_SIZE_DOCS}`
}

function createPayloadTooLargeError(size: number, functionId?: string): InvocationError {
  return new InvocationError({
    code: 'payload_too_large',
    message: payloadTooLargeMessage(size),
    function_id: functionId,
  })
}

function isPayloadTooLargeError(error: unknown): error is Error & { code: string } {
  return error instanceof Error && 'code' in error && error.code === 'payload_too_large'
}

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

/** Why `call` refuses a call through an observer that names metadata (v2 §4.6, decision 53); the same in every SDK. */
const OBSERVER_CALL_METADATA =
  "a call through an observer takes no metadata; its target receives the observer's metadata"

/**
 * Configuration options passed to {@link registerWorker}.
 *
 * @example
 * ```typescript
 * const worker = registerWorker('ws://localhost:49135', {
 *   invocationTimeoutMs: 10000,
 *   reconnectionConfig: { maxRetries: 5 },
 * })
 * ```
 */
export type InitOptions = {
  /**
   * Name this worker announces to the engine. Defaults to a unique
   * `browser:<random>` per client: a browser has no pid to disambiguate by, and
   * the engine allows only one live worker per name in a namespace, so two tabs
   * sharing a fixed name would evict each other.
   */
  workerName?: string
  /**
   * Namespace this worker registers under. When omitted the engine applies its
   * `default` namespace. Scopes worker and function registrations so
   * identically-named entries can coexist across namespaces. The browser has no
   * `process.env`, so there is no environment-variable fallback: pass the
   * option explicitly.
   */
  namespace?: string
  /** Default timeout for `worker.call()` invocations in milliseconds. Defaults to `30000`. */
  invocationTimeoutMs?: number
  /**
   * WebSocket reconnection behavior.
   *
   * @see {@link IIIReconnectionConfig} for available fields and defaults.
   */
  reconnectionConfig?: Partial<IIIReconnectionConfig>
  /** Browser WebSocket connections authenticate via query parameters or cookies; the `headers` option is ignored. */
  headers?: Record<string, string>
  /** @internal Test knobs for the observe hooks and the reporter. */
  observe?: { hookTimeoutMs?: number; reporter?: Partial<ReporterLimits> }
}

/** Short random suffix; `crypto.randomUUID` is unavailable on insecure origins. */
function randomId(): string {
  const c = globalThis.crypto
  if (c?.randomUUID) {
    return c.randomUUID().slice(0, 8)
  }
  if (c?.getRandomValues) {
    return Array.from(c.getRandomValues(new Uint8Array(4)))
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('')
  }
  return Math.random().toString(16).slice(2, 10)
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
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(value)
    } catch {
      // not cloneable: fall back to what the wire would carry
    }
  }
  const text = JSON.stringify(value)
  return (text === undefined ? undefined : JSON.parse(text)) as T
}

const UNSUPPORTED_REASON = {
  code: 'UNSUPPORTED',
  message: 'the engine does not support observe (no observe.v1 in its features); upgrade it to 0.25 or later',
}

/** What an outgoing `invokefunction` carries from the invocation it is made in. */
type OutgoingFields = { caller?: CallerWire; observe_depth?: number; traceparent?: string; baggage?: string }

/** A call the engine routes: by function id, or to the target of an observer (`activation`). */
type RemoteRequest = {
  function_id: string
  payload: unknown
  action?: CallActionType
  timeoutMs?: number
  metadata?: unknown
  traceparent?: string
  baggage?: string
}

/** How a handler finished: what it returned, or what it threw. */
type HandlerOutcome = { ok: true; value: unknown } | { ok: false; error: unknown }

function resolveNamespace(optionNamespace?: string): string | undefined {
  if (optionNamespace !== undefined && optionNamespace.trim() === '') {
    throw new Error(
      `namespace is empty: options.namespace was set to ${JSON.stringify(optionNamespace)}. ` +
        'Give it a name, or leave it unset to register in `default`.',
    )
  }
  return optionNamespace
}

class Sdk implements ISdk {
  private ws?: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private invocations = new Map<string, Invocation & { timeout?: ReturnType<typeof setTimeout> }>()
  private messagesToSend: Record<string, unknown>[] = []
  private reconnectTimeout?: ReturnType<typeof setTimeout>
  private invocationTimeoutMs: number
  private reconnectionConfig: IIIReconnectionConfig
  private reconnectAttempt = 0
  private connectionState: IIIConnectionState = 'disconnected'
  private connectionListeners = new Set<(state: IIIConnectionState) => void>()
  private isShuttingDown = false
  private readonly workerName: string
  private readonly namespace?: string
  private workerId?: string
  private reattachToken?: string
  private fatalError?: RegistrationRejectedError
  // Observers this worker created, replayed (same id) on every connection:
  // the handle that holds each id (decision 56).
  private observerHandles = new Map<string, ObserverHandleImpl>()
  // Per id, the handles whose `observe` went out on this connection, oldest
  // first: the engine's answers name an id, and their order says which of
  // them each answer is about (decision 56). A disposed handle leaves once it
  // has its `observeresult`; the holder of the id is the last.
  private observesSent = new Map<string, ObserverHandleImpl[]>()
  // The engine's features (`WorkerRegistered.features`), kept across a
  // disconnect until the next `WorkerRegistered` replaces them.
  private engineFeatures: ReadonlySet<string> = new Set()
  // `WorkerRegistered` arrived on the current connection.
  private registeredOnConnection = false
  // Bumped on every connection: `functionregistered` confirms a function on one.
  private connectionEpoch = 0
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
  private standbyTimers = new Map<string, ReturnType<typeof setTimeout>>()
  // Observe-family frames waiting for the next `WorkerRegistered` (see sendObserveFamily).
  private observeQueue: Record<string, unknown>[] = []
  private readonly reporter: Reporter
  private readonly observeHost: ObserveHost

  constructor(
    private readonly address: string,
    options?: InitOptions,
  ) {
    this.workerName = options?.workerName ?? `browser:${randomId()}`
    // No env fallback: the browser has no `process.env`, so the option is the
    // only source -- and the only thing that can be wrong.
    this.namespace = resolveNamespace(options?.namespace)
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
        // Handed to the socket and not written yet: counts against the reporter's bound.
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

    this.reporter.start()
    this.connect()
  }

  private logError(message: string, error?: unknown): void {
    if (error === undefined) console.error(`[iii] ${message}`)
    else console.error(`[iii] ${message}`, error)
  }

  private logWarn(message: string): void {
    console.warn(`[iii] ${message}`)
  }

  private logInfo(message: string): void {
    console.info(`[iii] ${message}`)
  }

  /**
   * Registers a function with the engine. The `functionId` is the unique
   * identifier callers and observers name. The handler's return value is what
   * the function's observers observe.
   *
   * The handler runs locally in the browser session; HTTP invocation configs
   * are a Node.js SDK feature and are not accepted here. It receives the
   * payload and the invocation's metadata. Calls this worker makes to it run
   * in-process once the engine confirmed it, unless `options.localDispatch`
   * is `false`.
   *
   * A function that sets `args_format`, `private: true`, `observable: false`,
   * an `onObserverMatch` hook or an `onObserverDeliver` hook needs an engine
   * with `observe.v2`: it is registered once the connection's engine
   * advertised it, and against an older engine it stays unregistered (logged
   * once, `UNSUPPORTED`).
   *
   * @param functionId - Unique function identifier.
   * @param handler - Handler executed when the function is invoked.
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
    handler: RemoteFunctionHandler<I, O>,
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
    const fullMessage = this.snapshotRegistration<RegisterFunctionMessage>({
      ...wireOptions,
      ...(isPrivate === true ? { private: true } : {}),
      ...(observable === false ? { observable: false } : {}),
      id: functionId,
      message_type: MessageType.RegisterFunction,
    })
    // Refused here when too large, before anything is retained.
    const frame = this.prepareJson(this.toWireFormat(MessageType.RegisterFunction, fullMessage))

    const observe = new FunctionObservers(functionId, localDispatch !== false, true, this.observeHost)
    const fn: RemoteFunctionData = {
      message: fullMessage,
      observe,
      standby: standby === true,
      handler: async (input, metadata?: unknown) => {
        return await handler(input, metadata)
      },
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
      // This registration's observers: one load, no lookup by id.
      observers: () => observe.observers(),
      isObserved: (call) => observe.isObserved(call as Partial<Call>),
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
      `UNSUPPORTED: function "${functionId}" needs observe.v2 (args_format, private, observable: false, onObserverMatch or onObserverDeliver), which this engine does not advertise; it stays unregistered until the worker connects to an engine with observe.v2.`,
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
  onObserverAdded: ISdk['onObserverAdded'] = (a: string | AddedHook | null, b?: AddedHook | null) => {
    if (typeof a === 'string') this.setFunctionHook(a, 'added', b as AddedHook | null)
    else this.setWorkerHook('added', a)
    return this
  }

  /**
   * Runs `hook` once for every accepted observer when it is removed; with a
   * function id, for that function only. When both are set, the function's
   * runs first. `null` removes the hook.
   */
  onObserverRemoved: ISdk['onObserverRemoved'] = (a: string | RemovedHook | null, b?: RemovedHook | null) => {
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
  onObserverMatch: ISdk['onObserverMatch'] = (functionId: string, hook: MatchHook | null): ISdk => {
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
  onObserverDeliver = <I = any, O = any, R = O>(functionId: string, hook: DeliverHook<I, O, R> | null): ISdk => {
    this.setFunctionHook(functionId, 'deliver', hook as DeliverHook | null)
    return this
  }

  /** The active observers of a function this worker hosts (empty for any other). */
  observers = (functionId: string): readonly Observer[] => this.functions.get(functionId)?.observe.observers() ?? []

  /**
   * Whether a call of a function this worker hosts would reach anyone (see
   * {@link ISdk.isObserved}).
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
   * and reject `ready()`. The handle holds its id until it is disposed: an
   * `observe` of an id a handle of this client holds throws an
   * {@link ObserveRejection} (`OBSERVER_ID_CONFLICT`) and sends nothing.
   *
   * @param source - What is observed: a function id, or `function_id`,
   *   `args`, `filters`, and `on` / `capture` / `deliver`.
   * @param target - The function to invoke (registered first; until then the observer is `pending`).
   * @param options - Policy and metadata: `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * worker.registerFunction('ui::order-shipped', async ({ input }: Observed) => showToast(input))
   *
   * const shipped = worker.observe(
   *   { function_id: 'orders::ship', filters: { 'input.customer_id': { $eq: customerId } }, capture: ['input'] },
   *   'ui::order-shipped',
   * )
   * await shipped.ready()
   * // later
   * await shipped.dispose()
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
    const id = options?.id ?? randomUUID()
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
   * Browsers have no `AsyncLocalStorage`: it is known while the handler runs
   * synchronously, up to its first `await`. Read it first and re-enter it with
   * {@link withInvocation} for what the handler does after an `await`. Calls
   * made after an `await` without it still carry the deepest running
   * invocation's `observe_depth`, so the loop guard holds (see `withInvocation`).
   *
   * @example
   * ```typescript
   * worker.registerFunction('ui::checkout', async (cart) => {
   *   const inv = worker.currentInvocation() // { function_id, invocation_id, caller, observe_depth, ... }
   *   const quote = await fetchQuote(cart)
   *   return worker.withInvocation(inv, () => worker.call({ function_id: 'orders::create', payload: quote }))
   * })
   * ```
   */
  currentInvocation = (): InvocationContext | undefined => publicContext(currentContext())

  /**
   * Runs `fn` inside `invocation` (what {@link currentInvocation} returned) and
   * returns its result: the calls `fn` makes before its own first `await`
   * carry that invocation's caller, `observe_depth` and `traceparent`. When
   * `fn` returns a promise, the invocation counts as running until it settles.
   * `undefined` runs `fn` outside any invocation.
   *
   * Code after an `await`, outside `withInvocation`, runs outside any
   * invocation, but its calls are not treated as new chains: they carry the
   * `observe_depth` of the deepest invocation running on this page, so the
   * engine's loop guard (`max_observe_depth`) still stops an observer cycle
   * whose target awaits before calling back. With several invocations
   * running this over-counts, and a legitimate chain may stop early. They
   * carry a `caller` and `traceparent` only when that is unambiguous (one
   * invocation is running, and it is an observe activation or its function's
   * deliver hook is running); otherwise none. An invocation that never
   * settles stops counting as running after 10 minutes (a local call: at its
   * timeout). Local calls nest the same way: a self-recursion that awaits
   * before each call goes through the engine past 64 levels, while calls
   * started together count as one level. A limitation: local calls started in
   * separate ticks while earlier ones still run also look nested, so a long,
   * wide fan-out may send some through the engine (same results). While the
   * socket is closed none is: a function this worker hosts then runs here up
   * to 256 local calls deep, and past that the call fails at once with
   * `LOCAL_DEPTH_EXCEEDED` (a runaway recursion must not freeze the page).
   */
  withInvocation = <T>(invocation: InvocationContext | undefined, fn: () => T): T =>
    enterInvocation(contextFromView(invocation), fn, MAX_RUNNING_INVOCATION_MS)

  private sendObserve(handle: ObserverHandleImpl): void {
    this.sendMessage(MessageType.Observe, handle.message, true)
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
      this.sendMessage(type, message as Omit<IIIMessage, 'message_type'>, true)
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
   * `iii-browser-sdk/helpers` submodule. Not part of the public `ISdk` surface.
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
      writer: new ChannelWriter(this.address, result.writer),
      reader: new ChannelReader(this.address, result.reader),
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
   * payload goes as given. It is routed by the observer alone, so it reaches
   * the observer's target from anywhere, after an `await` included. It takes
   * no metadata: the target's metadata argument is the observer's
   * registration metadata, and a request that names metadata is refused
   * before anything is sent.
   *
   * @param request - The call request.
   * @param request.function_id - ID of the function to invoke.
   * @param request.observer - An observer of a function this worker hosts: its target is invoked.
   * @param request.payload - Payload to pass to the function.
   * @param request.action - Routing action. Omit for synchronous request/response.
   * @param request.timeoutMs - Override the default invocation timeout.
   * @param request.metadata - Per-invocation metadata (any JSON value), delivered to the handler
   *   beside the payload. `null` is the same as no metadata: the handler receives `undefined`.
   * @param request.traceparent - W3C `traceparent` to continue; defaults to the current invocation's.
   * @param request.dispatch - `remote` forces the engine path for a function this worker hosts.
   * @returns The result of the function invocation.
   *
   * A call to a function this worker registered runs in-process once the
   * engine confirmed it for local dispatch (and puts nothing on the wire
   * unless someone observes the function); `enqueue` and `dispatch: 'remote'`
   * always go through the engine.
   *
   * @example
   * ```typescript
   * import { CallAction } from 'iii-browser-sdk'
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
   * worker.onObserverDeliver('ui::on-command', async (call) => {
   *   const handler = commands.match(call.input)
   *   return handler ? worker.call({ observer: handler.obs, payload: call.input }) : { handled: false }
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  call = async <TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput> => {
    if (request.observer !== undefined) {
      const { observer, payload, action, timeoutMs } = request
      // Its target's metadata is the observer's: refused before anything is
      // sent, so no metadata is lost on the way (decision 53). `null` is none.
      if ((request as { metadata?: unknown }).metadata != null) throw new Error(OBSERVER_CALL_METADATA)
      // The engine routes to the observer's stored target and is the one that
      // checks it (decision 13); no namespace travels.
      return this.invokeRemote<TOutput>({ function_id: observer.target, payload, action, timeoutMs }, undefined, {
        observer_id: observer.id,
      })
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
    // so does a call already MAX_LOCAL_DEPTH local calls deep. While the socket
    // is closed the engine cannot take it, so it runs here up to
    // MAX_OFFLINE_LOCAL_DEPTH, and past that it fails at once.
    if (req.dispatch !== 'remote' && action?.type !== 'enqueue') {
      const fn = this.functions.get(function_id)
      const observe = fn?.observe
      if (fn?.handler && observe?.localDispatch && (namespace === undefined || namespace === observe.namespace)) {
        const depth = callLocalDepth()
        if (depth < MAX_LOCAL_DEPTH) return this.invokeLocal<TOutput>(function_id, fn, req)
        if (!this.isOpen()) {
          if (depth < MAX_OFFLINE_LOCAL_DEPTH) return this.invokeLocal<TOutput>(function_id, fn, req)
          throw new InvocationError({
            code: 'LOCAL_DEPTH_EXCEEDED',
            message: `${function_id} is called ${depth} local calls deep: past ${MAX_LOCAL_DEPTH} the call needs the engine, and the socket is closed`,
            function_id,
          })
        }
      }
    }
    return this.invokeRemote<TOutput>(req, namespace)
  }

  /**
   * Invokes a function using a request object.
   *
   * @deprecated Use {@link Sdk.call}, the same function.
   */
  trigger = this.call

  /**
   * What an outgoing call carries from the invocation it is made in: `caller`
   * and `observe_depth` (engines with `observe.v1` only), and the trace
   * context. The browser has no OpenTelemetry, so `traceparent` / `baggage`
   * are the request's own, else the invocation's (see `callContext`), else
   * none. Outside any scope `observe_depth` is the deepest running
   * invocation's (`callDepth`), so the loop guard holds after an `await`.
   */
  private outgoingFields(request: { traceparent?: string; baggage?: string }): OutgoingFields {
    const cur = callContext()
    const fields: OutgoingFields = {
      traceparent: request.traceparent ?? cur?.traceparent,
      baggage: request.baggage ?? cur?.baggage,
    }
    if (this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      if (cur) fields.caller = callerFor(cur)
      fields.observe_depth = cur ? cur.observe_depth : callDepth()
    }
    return fields
  }

  /** The routed path: an `InvokeFunction` through the engine. */
  private invokeRemote<TOutput>(
    request: RemoteRequest,
    namespace: string | undefined,
    activation?: ActivationWire,
  ): Promise<TOutput> {
    // The engine refused this worker for good: nothing reaches it.
    if (this.fatalError) return Promise.reject(this.fatalError)
    const { function_id, payload, action, timeoutMs, metadata } = request
    const effectiveTimeout = timeoutMs ?? this.invocationTimeoutMs
    const outgoing = this.outgoingFields(request)

    if (action?.type === 'void') {
      this.sendMessage(
        MessageType.InvokeFunction,
        buildInvokeFunction({
          function_id,
          data: payload,
          action,
          metadata,
          // Omitted only when this worker has no namespace either.
          namespace,
          ...outgoing,
          activation,
        }) as Omit<IIIMessage, 'message_type'>,
      )
      return Promise.resolve(undefined as TOutput)
    }

    const invocation_id = randomUUID()

    return new Promise<TOutput>((resolve, reject) => {
      const timeout = setTimeout(() => {
        const invocation = this.invocations.get(invocation_id)
        if (invocation) {
          this.invocations.delete(invocation_id)
          reject(new InvocationTimeoutError(effectiveTimeout, function_id))
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

      this.sendMessage(
        MessageType.InvokeFunction,
        buildInvokeFunction({
          invocation_id,
          function_id,
          data: payload,
          action,
          metadata,
          // Omitted only when this worker has no namespace either.
          namespace,
          ...outgoing,
          activation,
        }) as Omit<IIIMessage, 'message_type'>,
      )
    })
  }

  /**
   * Runs a call's delivery inside `ctx`: the function's deliver hook, or
   * iii's delivery to its observers. The hook sees the invocation up to its
   * first `await`, and counts as delivering until it settles, so the calls it
   * makes after an `await` fall back to the invocation while it is the only
   * one running (`callContext`).
   */
  private deliverIn(ctx: ActiveInvocation, delivery: Delivery, hook: DeliverHook | undefined): Promise<unknown> {
    if (hook === undefined) return delivery.run(undefined)
    const untrack = trackDelivering(ctx)
    return runInContext(ctx, () => delivery.run(hook)).finally(untrack)
  }

  /**
   * The local path (sdk.md §4.3): runs the handler in-process inside a new
   * invocation context, then delivers the call (the function's deliver hook,
   * or iii's delivery to its observers); the same timeout and the same
   * rejection value as the remote path. A call of a function nobody observes
   * and without a deliver hook puts nothing on the wire.
   *
   * The handler gets a copy of the payload and the caller a copy of the
   * result; a private function's input and result pass by reference.
   */
  private invokeLocal<TOutput>(
    function_id: string,
    fn: RemoteFunctionData,
    request: {
      payload: unknown
      action?: CallActionType
      timeoutMs?: number
      metadata?: unknown
      traceparent?: string
      baggage?: string
    },
  ): Promise<TOutput> {
    const { observe } = fn
    const handler = fn.handler as NonNullable<RemoteFunctionData['handler']>
    const parent = callContext()
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
    const ctx: ActiveInvocation = {
      function_id,
      invocation_id: randomUUID(),
      caller,
      observe_depth: parent ? parent.observe_depth : callDepth(),
      localDepth: callLocalDepth() + 1,
    }
    const traceparent = request.traceparent ?? parent?.traceparent
    const baggage = request.baggage ?? parent?.baggage
    if (traceparent !== undefined) ctx.traceparent = traceparent
    if (baggage !== undefined) ctx.baggage = baggage

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
    const settle = (handled: HandlerOutcome): unknown => {
      const duration = performance.now() - t0
      this.reporter.recordLocal(function_id, handled.ok, duration)
      if (!delivering) {
        if (handled.ok) return handled.value
        throw handled.error
      }
      const outcome = byReference ? handled : this.deliveredOutcome(handled, input, callInput)
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
      return this.deliverIn(ctx, delivery, hook)
    }

    const effectiveTimeout = request.timeoutMs ?? this.invocationTimeoutMs
    // Running until the call is delivered: what code after its `await`s falls back to.
    const untrack = trackRunning(ctx, effectiveTimeout)
    let started: Promise<unknown>
    try {
      started = runInContext(ctx, () => handler(input, metadata))
    } catch (error) {
      // Thrown before the handler's body ran (a stack too deep to enter it).
      started = Promise.reject(error)
    }
    const done = started
      .then(
        (value) => settle({ ok: true, value }),
        (error) => settle({ ok: false, error }),
      )
      .then(
        (output) => {
          untrack()
          return output
        },
        (error) => {
          untrack()
          // What the remote path rejects with: the error body, as the engine forwards it.
          throw delivery ? delivery.errorBody(error) : handlerErrorBody(error)
        },
      )

    if (request.action?.type === 'void') {
      done.catch((error) => this.logError(`Error invoking function ${function_id}`, error))
      return Promise.resolve(undefined as TOutput)
    }

    return new Promise<TOutput>((resolve, reject) => {
      // The handler's outcome is handled before anything else can fail here,
      // so it is never left as an unhandled rejection.
      let timeout: ReturnType<typeof setTimeout> | undefined
      done.then(
        (output) => {
          if (timeout !== undefined) clearTimeout(timeout)
          try {
            resolve((byReference ? output : cloneValue(output)) as TOutput)
          } catch (error) {
            reject(error)
          }
        },
        (error) => {
          if (timeout !== undefined) clearTimeout(timeout)
          reject(error)
        },
      )
      timeout = setTimeout(() => {
        reject(new InvocationTimeoutError(effectiveTimeout, function_id))
      }, effectiveTimeout)
    })
  }

  /**
   * The outcome a local call of a function that is not private is delivered
   * with: a handler that returns its own input unchanged is delivered with
   * the input the call started with as its output, so the report carries the
   * value once (`output_is_input`, decision 4).
   */
  private deliveredOutcome(outcome: HandlerOutcome, input: unknown, callInput: unknown): HandlerOutcome {
    if (!outcome.ok) return outcome
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

  /**
   * @internal Implementation backing the `createStream` helper in the
   * `iii-browser-sdk/helpers` submodule. Not part of the public `ISdk` surface.
   *
   * Registers a custom stream implementation, overriding the engine default
   * for the given stream name.
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
   * The terminal rejection that closed this connection for good, if any. Set
   * only for a non-retryable registration rejection (e.g. a worker-name
   * collision). Mirrors the Node SDK.
   */
  getFatalError = (): RegistrationRejectedError | undefined => this.fatalError

  /**
   * Gracefully shutdown the SDK, cleaning up all resources.
   */
  shutdown = async (): Promise<void> => {
    this.isShuttingDown = true

    // Last chance for buffered reports and counters to reach the engine.
    try {
      this.reporter.flush()
      this.reporter.sendStats()
    } catch (error) {
      this.logError('Failed to flush observed invocations on shutdown', error)
    }
    this.reporter.stop()

    this.clearReconnectTimeout()
    for (const functionId of [...this.standbyTimers.keys()]) this.stopStandby(functionId)

    for (const [_id, invocation] of this.invocations) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      invocation.reject(new Error('iii is shutting down'))
    }
    this.invocations.clear()

    if (this.ws) {
      this.ws.onopen = null
      this.ws.onclose = null
      // Closing a socket that is still connecting reports an error: ignore it,
      // this is a shutdown. (Without a handler, `ws`, which runs this SDK in
      // Node, throws it as an uncaught exception.)
      this.ws.onerror = () => undefined
      this.ws.onmessage = null
      try {
        this.ws.close()
      } catch {
        // shutting down anyway
      }
      this.ws = undefined
    }

    this.setConnectionState('disconnected')
  }

  /**
   * Subscribe to connection-state transitions. The handler is fired immediately
   * with the current state, then on every transition. Multiple listeners are
   * supported. Returns an unsubscribe function.
   */
  addConnectionStateListener = (handler: (state: IIIConnectionState) => void): (() => void) => {
    this.connectionListeners.add(handler)
    try {
      handler(this.connectionState)
    } catch (e) {
      console.error('[iii] connection listener threw on initial fire', e)
    }
    return () => {
      this.connectionListeners.delete(handler)
    }
  }

  // private methods

  private setConnectionState(state: IIIConnectionState): void {
    if (this.connectionState !== state) {
      this.connectionState = state
      for (const handler of this.connectionListeners) {
        try {
          handler(state)
        } catch (e) {
          console.error('[iii] connection listener threw', e)
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
    this.ws.onopen = this.onSocketOpen.bind(this)
    this.ws.onclose = this.onSocketClose.bind(this)
    this.ws.onerror = this.onSocketError.bind(this)
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
      console.error(`[iii] Max reconnection retries (${maxRetries}) reached, giving up`)
      return
    }

    if (this.reconnectTimeout) {
      return
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

  private onSocketError(): void {
    console.error('[iii] WebSocket error')
  }

  private onSocketClose(): void {
    if (this.ws) {
      this.ws.onopen = null
      this.ws.onclose = null
      this.ws.onerror = null
      this.ws.onmessage = null
    }
    this.ws = undefined
    // Local dispatch and the compiled observer sets survive; what was sent on
    // this connection is replayed on the next one (standby functions too).
    this.registeredOnConnection = false
    this.forgetObservesSent()
    for (const functionId of [...this.standbyTimers.keys()]) this.stopStandby(functionId)

    // A fatal registration rejection (or shutdown) sets `isShuttingDown` and a
    // terminal state before closing the socket. Do not overwrite that state with
    // `disconnected`, and do not schedule a reconnect the engine would only
    // reject again. `scheduleReconnect` already no-ops while shutting down.
    if (this.isShuttingDown) {
      return
    }

    this.setConnectionState('disconnected')
    this.scheduleReconnect()
  }

  private onSocketOpen(): void {
    this.clearReconnectTimeout()
    this.reconnectAttempt = 0
    this.setConnectionState('connected')
    this.connectionEpoch++
    this.registeredOnConnection = false
    this.forgetObservesSent()

    if (this.ws) {
      this.ws.onmessage = this.onMessage.bind(this)
    }

    // Reconnect: present the previous engine-assigned identity BEFORE the
    // metadata announce and registration replay so the engine retires the old
    // connection and the replay lands on a clean slate instead of racing its
    // cleanup. This must precede registerWorkerMetadata(): otherwise the old
    // connection still holds this (namespace, worker_name) and the announce
    // would trip WORKER_NAMESPACE_CONFLICT against ourselves. The token proves
    // we ARE that worker (ids alone are publicly listable).
    if (this.workerId) {
      try {
        this.sendMessageRaw(
          JSON.stringify({
            type: MessageType.Reattach,
            previous_worker_id: this.workerId,
            reattach_token: this.reattachToken,
          }),
        )
      } catch (error) {
        if (!isPayloadTooLargeError(error)) throw error
        console.error('[iii] Reattach JSON envelope rejected', error)
      }
    }

    // Announce before registering anything. The engine buffers a connection's
    // registrations until it knows the connection's namespace, which it takes
    // from this call — or, failing that, only after a grace period. Registering
    // first would stall every function behind that grace.
    this.registerWorkerMetadata()

    // Functions that need observe.v2 wait for `WorkerRegistered`, and so do
    // the observers, which follow the functions (so local targets exist first).
    this.sendRegistrations()

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
      try {
        this.sendMessageRaw(JSON.stringify(message))
      } catch (error) {
        if (!isPayloadTooLargeError(error)) throw error
        console.error('[iii] Queued JSON envelope rejected', error)
      }
    }
  }

  private isOpen(): boolean {
    return this.ws?.readyState === WebSocket.OPEN
  }

  private snapshotRegistration<T extends object>(message: T): T {
    return JSON.parse(JSON.stringify(message)) as T
  }

  private replayRegistration(type: MessageType, message: Omit<IIIMessage, 'message_type'>): void {
    try {
      this.sendMessage(type, message, true)
    } catch (error) {
      if (!isPayloadTooLargeError(error)) throw error
      console.error('[iii] Registration JSON envelope rejected', error)
    }
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

  private prepareJson(message: Record<string, unknown>): string {
    const data = JSON.stringify(message)
    const size = JSON_ENCODER.encode(data).byteLength
    if (size <= MAX_JSON_FRAME_BYTES) return data

    const error = createPayloadTooLargeError(
      size,
      typeof message.function_id === 'string' ? message.function_id : undefined,
    )
    if (message.type === MessageType.InvocationResult) {
      // The wire body carries the bare message: `error.message` already has the `code: ` prefix.
      const fallback = JSON.stringify(
        this.oversizedAnswer(message, {
          type: MessageType.InvocationResult,
          invocation_id: message.invocation_id,
          function_id: message.function_id,
          error: { code: error.code, message: payloadTooLargeMessage(size) },
        }),
      )
      if (JSON_ENCODER.encode(fallback).byteLength <= MAX_JSON_FRAME_BYTES) return fallback
    }

    if (typeof message.invocation_id === 'string') {
      const pending = this.invocations.get(message.invocation_id)
      this.invocations.delete(message.invocation_id)
      pending?.reject(error)
    }
    throw error
  }

  private sendMessageRaw(data: string): void {
    if (JSON_ENCODER.encode(data).byteLength > MAX_JSON_FRAME_BYTES) {
      data = this.prepareJson(JSON.parse(data) as Record<string, unknown>)
    }
    if (this.ws && this.isOpen()) {
      try {
        this.ws.send(data)
      } catch (error) {
        console.error('[iii] Exception while sending message', error)
      }
    }
  }

  private toWireFormat(messageType: MessageType, message: Omit<IIIMessage, 'message_type'>): Record<string, unknown> {
    const { message_type: _, ...rest } = message as Record<string, unknown>
    return { type: messageType, ...rest } as Record<string, unknown>
  }

  /**
   * Tells the engine who this connection is. Sent on every open, before any
   * registration, so the engine can resolve this connection's namespace and
   * drain its buffered registrations immediately.
   */
  private registerWorkerMetadata(): void {
    this.sendMessage(MessageType.InvokeFunction, {
      function_id: EngineFunctions.REGISTER_WORKER,
      data: {
        runtime: 'browser',
        name: this.workerName,
        os: globalThis.navigator?.userAgent ?? 'browser',
        // Omit when absent so the engine falls back to its `default` namespace.
        ...(this.namespace !== undefined ? { namespace: this.namespace } : {}),
        // Opts this connection into the observe messages (CONTRACT D2).
        features: [...SDK_FEATURES],
      },
      action: { type: 'void' },
    } as unknown as Omit<IIIMessage, 'message_type'>)
  }

  private sendMessage(messageType: MessageType, message: Omit<IIIMessage, 'message_type'>, skipIfClosed = false): void {
    const wireMessage = this.toWireFormat(messageType, message)
    const data = this.prepareJson(wireMessage)
    if (this.isOpen()) {
      this.sendMessageRaw(data)
    } else if (!skipIfClosed) {
      this.messagesToSend.push(JSON.parse(data) as Record<string, unknown>)
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

  /**
   * Replaces channel refs with readers / writers. Copy-on-write: a value
   * without refs is returned as it is. Only arrays and plain objects are
   * walked, so a locally dispatched payload keeps its `Date`s, `Map`s and
   * class instances.
   */
  private resolveChannelValue(value: unknown): unknown {
    if (isChannelRef(value)) {
      return value.direction === 'read'
        ? new ChannelReader(this.address, value)
        : new ChannelWriter(this.address, value)
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
    metadata?: unknown,
    traceparent?: string,
    baggage?: string,
    observeFields: {
      caller?: CallerWire
      observe_depth?: number
      activation?: ActivationWire
    } = {},
  ): Promise<unknown> {
    const fn = this.functions.get(function_id)

    if (fn?.handler) {
      const handler = fn.handler
      // The invocation context every handler sees (`currentInvocation()`), and
      // what the calls it makes carry as their caller and trace context.
      const ctx: ActiveInvocation = {
        function_id,
        invocation_id: invocation_id ?? randomUUID(),
        observe_depth: observeFields.observe_depth ?? 0,
      }
      if (observeFields.caller) ctx.caller = observeFields.caller
      if (observeFields.activation) ctx.activation = observeFields.activation
      if (traceparent !== undefined) ctx.traceparent = traceparent
      if (baggage !== undefined) ctx.baggage = baggage

      // The engine evaluates the `on` and filters of the calls it routes
      // (decision 55). An observe.v2 host runs the function's match and
      // deliver hooks for them, which see the input as it arrived (channel
      // refs unresolved), and its answer says how the call is delivered:
      // `delivered`, `matched`, or neither for a function with no hook.
      const snapshot: readonly ObserverImpl[] = fn.observe.compiled
      const activations = fn.observe.activations
      const hook = this.functionHooks.get(function_id)?.deliver
      const answering = this.engineFeatures.has(FEATURE_OBSERVE_V2)
      const startedAt = Date.now()
      const t0 = performance.now()
      let delivery: Delivery | undefined
      // The answer's `matched` when the call needs no delivery.
      let matched: readonly string[] | undefined
      const resolvedInput = this.resolveChannelValue(input)
      // Running until the call is delivered: what code after its `await`s falls back to (`callDepth`).
      const run = async (): Promise<unknown> => {
        const untrack = trackRunning(ctx, MAX_RUNNING_INVOCATION_MS)
        try {
          let outcome: HandlerOutcome
          try {
            outcome = { ok: true, value: await runInContext(ctx, () => handler(resolvedInput, metadata)) }
          } catch (error) {
            outcome = { ok: false, error }
          }
          // No delivery runs without a deliver hook when the function has no
          // match hook (the answer carries neither field: the engine
          // evaluates every active observer), or when no observer was active
          // as the call started (`matched: []`: the match hook has none to
          // match and does not run, as a local call with an empty snapshot
          // skips delivery).
          const narrowed = answering && hook === undefined && fn.observe.hasMatchHook()
          if (!answering || (hook === undefined && (!narrowed || snapshot.length === 0))) {
            if (narrowed) matched = []
            if (outcome.ok) return outcome.value
            throw outcome.error
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
          return await this.deliverIn(ctx, delivery, hook)
        } finally {
          untrack()
        }
      }

      // Fire-and-forget (`void`, no invocation id): never answered. Activations
      // carry an id and are answered like any call (CONTRACT D16).
      if (!invocation_id) {
        try {
          await run()
        } catch (error) {
          console.error(`[iii] Error invoking function ${function_id}`, error)
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
            traceparent,
            baggage,
            matched: delivery ? delivery.matched : matched,
            delivered: delivery?.delivered,
          }) as Omit<IIIMessage, 'message_type'>,
        )
      } catch (error) {
        this.sendMessage(
          MessageType.InvocationResult,
          buildInvocationResult({
            invocation_id,
            function_id,
            error: delivery ? delivery.errorBody(error) : handlerErrorBody(error),
            traceparent,
            baggage,
            matched: delivery ? delivery.matched : matched,
            delivered: delivery?.delivered,
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

  private onMessage(event: MessageEvent): void {
    let msgType: MessageType
    let message: Record<string, unknown>

    try {
      const parsed = JSON.parse(typeof event.data === 'string' ? event.data : '') as Record<string, unknown>
      msgType = parsed.type as MessageType
      const { type: _, ...rest } = parsed
      message = rest
    } catch (error) {
      console.error('[iii] Failed to parse incoming message', error)
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
      // Only a registration sent on this connection is confirmed: not one
      // unregistered since (a match or deliver hook against an engine without observe.v2).
      if (fn && fn.sentEpoch === this.connectionEpoch) {
        // A standby function took over the id: its hooks run for every observer in the set.
        this.stopStandby(msg.id)
        fn.standingBy = false
        // Its reports held across a reconnect are flushed once the set is acked.
        fn.observe.onFunctionRegistered(msg, this.connectionEpoch)
      }
    } else if (msgType === MessageType.ObserverAdded) {
      const msg = decodeObserverAdded(message)
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
    } else if (msgType === MessageType.RegistrationRejected) {
      this.onRegistrationRejected(decodeRegistrationRejected(message))
    } else if (msgType === MessageType.WorkerRegistered) {
      const { worker_id, reattach_token, features } = decodeWorkerRegistered(message)
      this.workerId = worker_id
      this.reattachToken = reattach_token
      this.onEngineFeatures(features ?? [])
    }
  }

  /**
   * `WorkerRegistered` told this connection what the engine supports. The
   * functions that need `observe.v2` go out now, or stay unregistered without
   * it; without it, one sent earlier on this connection that has gained a
   * match or deliver hook since is unregistered. With `observe.v1` the observers are
   * replayed (after the functions) and held reports flushed; without it
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
      // What was queued while disconnected (an `unobserve`), then the replay.
      for (const frame of queued) this.sendMessageRaw(JSON.stringify(frame))
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

  /**
   * An `observeresult`: the answer to the oldest `observe` of the id sent on
   * this connection that has none yet. The engine answers every `observe`
   * once, in the order it read them (decision 56).
   */
  private onObserveResult(id: string, wireState: string, reason?: ErrorBody): void {
    const handle = this.observesSent.get(id)?.find((h) => !h.answered)
    if (!handle) return
    handle.answered = true
    const state = toObserverState(wireState)
    if (state === 'error' && !handle.created) {
      // A refusal, the handle's final answer: the engine stores nothing for
      // it and ignores its `unobserve`, so dispose() sends none.
      handle.sent = false
      this.unlistObserve(handle)
      handle.setState(state, reason ? { code: reason.code, message: reason.message } : undefined)
      return
    }
    // A replay's `error` answer for an observer the engine created may be
    // that observer's state (a reattached connection): it may hold it still.
    if (state !== 'error' && state !== 'disposed') handle.created = true
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

  /** A state from the engine for `handle`; `disposed` frees its id. */
  private applyObserverState(handle: ObserverHandleImpl, wireState: string, reason?: ErrorBody): void {
    const state = toObserverState(wireState)
    handle.setState(state, reason ? { code: reason.code, message: reason.message } : undefined)
    if (state !== 'disposed') return
    this.unlistObserve(handle)
    if (this.observerHandles.get(handle.id) === handle) this.observerHandles.delete(handle.id)
  }

  private unlistObserve(handle: ObserverHandleImpl): void {
    const sent = this.observesSent.get(handle.id)
    const at = sent ? sent.indexOf(handle) : -1
    if (!sent || at < 0) return
    sent.splice(at, 1)
    if (sent.length === 0) this.observesSent.delete(handle.id)
  }

  /**
   * The connection closed: its lists go. The engine may have created an
   * observer whose answer never arrived, and may hold it on a reattach.
   */
  private forgetObservesSent(): void {
    for (const sent of this.observesSent.values()) {
      for (const handle of sent) if (!handle.answered) handle.created = true
    }
    this.observesSent.clear()
  }

  /**
   * The engine rejected a registration. The `code` decides the severity:
   *
   * - {@link FUNCTION_NAMESPACE_CONFLICT}: another live worker in this
   *   namespace already exports this one function id. Only that registration
   *   is refused; the connection stays open and the worker keeps serving its
   *   other functions. A `standby` function retries its registration until
   *   the id is free.
   * - {@link FUNCTION_REJECTIONS} on one function: only that function is
   *   refused. Logged; the worker keeps serving.
   * - {@link WORKER_NAMESPACE_CONFLICT}, or any other code: terminal — stop and
   *   do not reconnect (otherwise the engine would only reject the same name
   *   again, which with `maxRetries: -1` loops forever).
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
        `Function registration rejected: function "${init.function_id ?? '<unknown>'}" in namespace "${init.namespace}" is already exported by worker ${init.owner_worker_id}. Staying connected and serving the rest.`,
      )
      return
    }
    if (FUNCTION_REJECTIONS.has(init.code) && init.function_id !== undefined) {
      this.functions.get(init.function_id)?.observe.onRegistrationRejected(this.connectionEpoch)
      this.logError(
        `Function registration rejected with ${init.code}: function "${init.function_id}" in namespace "${init.namespace}" is not registered. Staying connected and serving the rest.`,
      )
      return
    }

    if (init.code === WORKER_NAMESPACE_CONFLICT) {
      this.logError(
        `Registration rejected (${init.code}): worker "${init.worker_name}" in namespace "${init.namespace}" is already owned by ${init.owner_worker_id}. Not reconnecting.`,
      )
    } else {
      this.logError(
        `Registration rejected by engine with unknown code "${init.code}"; treating as fatal. Not reconnecting.`,
      )
    }
    // Terminal: suppress reconnect so neither onSocketClose nor scheduleReconnect
    // revives a connection the engine would only reject again.
    this.isShuttingDown = true
    this.clearReconnectTimeout()

    // Reject every in-flight invocation now rather than leaving them to time out.
    const error = new RegistrationRejectedError(init)
    this.fatalError = error
    for (const [, invocation] of this.invocations) {
      if (invocation.timeout) {
        clearTimeout(invocation.timeout)
      }
      invocation.reject(error)
    }
    this.invocations.clear()

    // Drop the socket. onSocketClose sees `isShuttingDown` and leaves the
    // terminal `failed` state in place instead of overwriting it.
    this.setConnectionState('failed')
    this.ws?.close()
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
    unrefTimer(timer)
    this.standbyTimers.set(functionId, timer)
  }

  private stopStandby(functionId: string): void {
    const timer = this.standbyTimers.get(functionId)
    if (!timer) return
    clearTimeout(timer)
    this.standbyTimers.delete(functionId)
  }
}

/**
 * Factory object that constructs routing actions for {@link ISdk.call}.
 *
 * @example
 * ```typescript
 * import { CallAction } from 'iii-browser-sdk'
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
 * @param address - WebSocket URL of the III engine (e.g. `ws://localhost:49135`).
 * @param options - Optional {@link InitOptions} for worker name, timeouts, and reconnection.
 * @returns A connected {@link ISdk} instance.
 *
 * @example
 * ```typescript
 * import { registerWorker } from 'iii-browser-sdk'
 *
 * const worker = registerWorker('ws://localhost:49135')
 * ```
 */
export const registerWorker = (address: string, options?: InitOptions): ISdk => new Sdk(address, options)
