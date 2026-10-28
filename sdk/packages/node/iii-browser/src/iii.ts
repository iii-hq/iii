import { ChannelReader, ChannelWriter } from './channels'
import {
  DEFAULT_BRIDGE_RECONNECTION_CONFIG,
  DEFAULT_INVOCATION_TIMEOUT_MS,
  EngineFunctions,
  type IIIConnectionState,
  type IIIReconnectionConfig,
} from './iii-constants'
import { InvocationError, RegistrationRejectedError } from './errors'
import { compileFilter, FilterError } from './filter'
import {
  type ActivationWire,
  type CallerWire,
  type ErrorBody,
  FEATURE_OBSERVE_V1,
  type IIIMessage,
  type InvocationResultMessage,
  MessageType,
  type RegisterFunctionMessage,
  type RegisterTriggerMessage,
  type RegisterTriggerTypeMessage,
  SDK_FEATURES,
  type StreamChannelRef,
  type TriggerRegistrationResultMessage,
  type TriggerRequest,
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
  trackRunning,
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
  Invocation,
  ISdk,
  RegisterFunctionOptions,
  RemoteFunctionData,
  RemoteFunctionHandler,
  RemoteTriggerTypeData,
  Trigger,
  TriggerTypeRef,
} from './types'
import { isChannelRef, randomUUID } from './utils'

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

function createPayloadTooLargeError(
  size: number,
  functionId?: string,
): InvocationError {
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
  /** Default timeout for `worker.trigger()` invocations in milliseconds. Defaults to `30000`. */
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
 * The worker's namespace, refusing one that was named and left blank.
 *
 * Absent and blank mean opposite things. Absent asks for the engine's
 * `default`; blank names a namespace and gives nothing to name it with. Read as
 * absent, the worker registers in `default`, and since a worker's calls and
 * triggers follow its namespace, the whole project quietly serves from a place
 * its declaration never named -- the one thing an operator cannot see by
 * reading the declaration.
 *
 * The browser has no `III_NAMESPACE` to fall back to, so unlike the other SDKs
 * there is only the option here, and an option that was written and left empty
 * is never what someone meant.
 */
/**
 * The namespace one call or one binding resolves in.
 *
 * `undefined` inherits the worker's. A blank one is refused: named and left
 * empty asks for the opposite of what absent asks for, and the two are only
 * ever confused by accident -- `??` forwards the empty string, Python's `or`
 * coerced it and Go dropped it, so one mistake produced three behaviours.
 */
function callNamespace(
  explicit: string | undefined,
  worker: string | undefined,
  source: string,
): string | undefined {
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

/** CONTRACT §5: one warning per page per deprecated API. */
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

/**
 * The error a call that timed out rejects with, on the remote and the local
 * path: an `Error` (its message unchanged from earlier releases) that also
 * carries `code: 'TIMEOUT'` and the `function_id`, like the Node SDK's.
 */
function timeoutError(ms: number, functionId: string): Error & { code: 'TIMEOUT'; function_id: string } {
  return Object.assign(new Error(`Invocation timeout after ${ms}ms: ${functionId}`), {
    code: 'TIMEOUT' as const,
    function_id: functionId,
  })
}

/** What an outgoing `invokefunction` carries from the invocation it is made in. */
type OutgoingFields = { caller?: CallerWire; observe_depth?: number; traceparent?: string; baggage?: string }

/** What a handler failure reports when the thrown value cannot even be read (a getter or `toString` that throws). */
const UNREADABLE_FAILURE = 'the handler failed with a value that cannot be read'

function safeString(value: unknown): string {
  try {
    return String(value)
  } catch {
    return UNREADABLE_FAILURE
  }
}

/**
 * An error body as the engine sends it, and as this SDK's own calls reject
 * with: a plain (JSON-parsed) object with a string `code`, a string `message`
 * and, optionally, a string `stacktrace`. A class instance or anything else
 * with such fields is not one: it is reported through `String()`.
 */
function isEngineErrorBody(value: unknown): value is { code: string; message: string; stacktrace?: string } {
  if (value === null || typeof value !== 'object') return false
  const proto = Object.getPrototypeOf(value)
  if (proto !== Object.prototype && proto !== null) return false
  const body = value as Record<string, unknown>
  return (
    typeof body.code === 'string' &&
    typeof body.message === 'string' &&
    (body.stacktrace === undefined || typeof body.stacktrace === 'string')
  )
}

/**
 * The error a handler's failure becomes, the same on the local and the remote
 * path (code `invocation_failed`):
 * - an `Error`, from any realm (an iframe's too): its message and stack;
 * - an engine error body (a failed call's rejection, rethrown): its message and stacktrace;
 * - anything else: `String(value)`.
 * Never throws: when reading the value throws, a fixed message is reported, so
 * the caller always gets its answer.
 */
function handlerErrorBody(error: unknown): ErrorBody {
  try {
    if (error instanceof Error || Object.prototype.toString.call(error) === '[object Error]') {
      const { message, stack } = error as Error
      return {
        code: 'invocation_failed',
        message: typeof message === 'string' ? message : safeString(message),
        stacktrace: typeof stack === 'string' ? stack : undefined,
      }
    }
    if (isEngineErrorBody(error)) {
      return { code: 'invocation_failed', message: error.message, stacktrace: error.stacktrace }
    }
    return { code: 'invocation_failed', message: String(error), stacktrace: undefined }
  } catch {
    return { code: 'invocation_failed', message: safeString(error), stacktrace: undefined }
  }
}

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
  private triggers = new Map<string, RegisterTriggerMessage>()
  private triggerTypes = new Map<string, RemoteTriggerTypeData>()
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
  // Observers this worker created, replayed (same id) on every connection.
  private observers = new Map<string, ObserverHandleImpl>()
  // The engine's features (`WorkerRegistered.features`), kept across a
  // disconnect until the next `WorkerRegistered` replaces them.
  private engineFeatures: ReadonlySet<string> = new Set()
  // `WorkerRegistered` arrived on the current connection.
  private registeredOnConnection = false
  // Bumped on every connection: `functionregistered` confirms a function on one.
  private connectionEpoch = 0
  private hooksInactiveWarned = new Set<string>()
  // Observe-family frames waiting for the next `WorkerRegistered` (see sendObserveFamily).
  private observeQueue: Record<string, unknown>[] = []
  private readonly reporter: Reporter
  private readonly observeHost: ObserveHost

  constructor(
    private readonly address: string,
    private readonly options?: InitOptions,
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
      identity: () =>
        this.workerId !== undefined
          ? { worker_id: this.workerId, worker_name: this.workerName }
          : { worker_name: this.workerName },
      sendAck: (ack) => this.sendMessage(MessageType.ObserverAck, buildObserverAck(ack), true),
      reporter: this.reporter,
      callTarget: (target, observerId, payload, timeoutMs) =>
        this.invokeRemote({ function_id: target, payload, timeoutMs }, undefined, {
          observer_id: observerId,
        }),
      logError: (message, error) => this.logError(message, error),
      hookTimeoutMs: () => options?.observe?.hookTimeoutMs ?? OBSERVE_HOOK_TIMEOUT_MS,
      // No OpenTelemetry here: an event emitted outside any handler starts no trace.
      traceContext: () => ({}),
    }

    this.reporter.start()
    this.connect()
  }

  private warnDeprecated(api: keyof typeof DEPRECATIONS): void {
    if (deprecationsWarned.has(api)) return
    deprecationsWarned.add(api)
    console.warn(`[iii] ${DEPRECATIONS[api]}`)
  }

  private logError(message: string, error?: unknown): void {
    if (error === undefined) console.error(`[iii] ${message}`)
    else console.error(`[iii] ${message}`, error)
  }

  /**
   * Registers a custom trigger type with the engine. A trigger type defines
   * how external events (HTTP, cron, queue, etc.) map to function invocations.
   *
   * @deprecated Register a function with an observe descriptor instead:
   * `registerFunction(id, handler, { observe: { delivery: 'owner', onObserverAdded, onObserverRemoved } })`.
   * Logs a warning once per page. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
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
      // Pairs a function with its trigger, so it defaults the trigger's namespace
      // to this worker's — otherwise the function lands in the worker's namespace
      // and the trigger in `default`, never resolving it. The low-level
      // `registerTrigger` keeps the engine default (`default`).
      registerTrigger: (functionId: string, config: TConfig) => {
        return this.registerTrigger({
          type: message.id,
          function_id: functionId,
          config,
          namespace: this.namespace,
        })
      },
      registerFunction: (functionId, handler, config) => {
        const ref = this.registerFunction(functionId, handler)
        this.registerTrigger({
          type: message.id,
          function_id: functionId,
          config,
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
    const { id, namespace } = triggerType as { id: string; namespace?: string }
    this.sendMessage(MessageType.UnregisterTriggerType, namespace !== undefined ? { id, namespace } : { id }, true)
    this.triggerTypes.delete(id)
  }

  /**
   * Binds a trigger configuration to a registered function. When the trigger
   * fires, the engine invokes the target function.
   *
   * @deprecated Use `iii.observe(source, target)` instead. Logs a warning once
   * per page. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
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
    const id = randomUUID()
    const fullTrigger = this.snapshotRegistration<RegisterTriggerMessage>({
      ...trigger,
      id,
      message_type: MessageType.RegisterTrigger,
      // Unset means this worker's namespace, not the engine's default. A
      // trigger names a function, and the function a worker registers lands in
      // the worker's namespace, so defaulting anywhere else registers a trigger
      // that fires and resolves nothing. Naming another namespace, `default`
      // included, stays a matter of saying so.
      ...(() => {
        const namespace = callNamespace(
          trigger.namespace,
          this.namespace,
          'RegisterTriggerInput.namespace',
        )
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
   * The handler runs locally in the browser session; HTTP invocation configs
   * are a Node.js SDK feature and are not accepted here. It receives the
   * payload and the invocation's metadata.
   *
   * Every function is observable. `options.observe` makes it observable on
   * its own terms (owner delivery, hooks when an observer is added or
   * removed), and calls this worker makes to it run in-process once the
   * engine confirmed it, unless `options.localDispatch` is `false`.
   *
   * @param functionId - Unique function identifier.
   * @param handlerOrInvocation - Async handler executed when the function is invoked.
   * @param options - Optional function registration options (description, request/response formats, metadata, observe hooks, localDispatch).
   * @returns A {@link FunctionRef} with `id`, `unregister()`, `isObserved()` and `observers()`.
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
    handlerOrInvocation: RemoteFunctionHandler,
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
    const fullMessage = this.snapshotRegistration<RegisterFunctionMessage>({
      ...wireOptions,
      ...(descriptor ? { observe: descriptor } : {}),
      id: functionId,
      message_type: MessageType.RegisterFunction,
    })

    this.sendMessage(MessageType.RegisterFunction, fullMessage, true)

    const observe = new FunctionObservers(functionId, hooks, localDispatch !== false, true, this.observeHost)
    if (observe.needsObserve && this.registeredOnConnection && !this.engineFeatures.has(FEATURE_OBSERVE_V1)) {
      this.warnHooksInactive(functionId)
    }

    const handler = handlerOrInvocation as RemoteFunctionHandler
    this.functions.set(functionId, {
      message: fullMessage,
      observe,
      handler: async (input, metadata?: unknown) => {
        return await handler(input, metadata)
      },
    })

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
  observe = (source: ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle => {
    if (!source || typeof source.function_id !== 'string' || source.function_id.trim() === '') {
      throw new Error('observe: source.function_id is required')
    }
    const targetId = typeof target === 'string' ? target : target?.function_id
    if (typeof targetId !== 'string' || targetId.trim() === '') {
      throw new Error('observe: target is required (the id of a registered function)')
    }
    const id = options?.id ?? randomUUID()
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
   *   return worker.withInvocation(inv, () => worker.trigger({ function_id: 'orders::create', payload: quote }))
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
   * invocation is running, and it is an observe activation); otherwise none.
   * They never get an invocation's metadata: `obs.emit` / `obs.call` filters
   * on `metadata.*` see it only inside a real scope. An invocation that never
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
      this.sendMessage(type, message as Omit<IIIMessage, 'message_type'>, true)
    }
  }

  private warnHooksInactive(functionId: string): void {
    if (this.hooksInactiveWarned.has(functionId)) return
    this.hooksInactiveWarned.add(functionId)
    console.warn(
      `[iii] Function "${functionId}" declares observe hooks or owner delivery, but the engine does not support observe (observe.v1): it is registered as a plain function and its observe hooks are inactive.`,
    )
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
   * @param request.metadata - Per-invocation metadata, delivered to the handler beside the payload.
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
   * import { TriggerAction } from 'iii-browser-sdk'
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
    // so does a call already MAX_LOCAL_DEPTH local calls deep. While the socket
    // is closed the engine cannot take it, so it runs here up to
    // MAX_OFFLINE_LOCAL_DEPTH, and past that it fails at once.
    if (request.dispatch !== 'remote' && action?.type !== 'enqueue') {
      const fn = this.functions.get(function_id)
      const observe = fn?.observe
      if (fn?.handler && observe?.localDispatch && (namespace === undefined || namespace === observe.namespace)) {
        const depth = callLocalDepth()
        if (depth < MAX_LOCAL_DEPTH) return this.invokeLocal<TOutput>(function_id, fn.handler, observe, request)
        if (!this.isOpen()) {
          if (depth < MAX_OFFLINE_LOCAL_DEPTH) {
            return this.invokeLocal<TOutput>(function_id, fn.handler, observe, request)
          }
          throw new InvocationError({
            code: 'LOCAL_DEPTH_EXCEEDED',
            message: `${function_id} is called ${depth} local calls deep: past ${MAX_LOCAL_DEPTH} the call needs the engine, and the socket is closed`,
            function_id,
          })
        }
      }
    }
    return this.invokeRemote<TOutput>(request, namespace)
  }

  /**
   * What an outgoing call carries from the invocation it is made in: `caller`
   * and `observe_depth` (engines with `observe.v1` only), and the trace
   * context. The browser has no OpenTelemetry, so `traceparent` / `baggage`
   * are the request's own, else the invocation's (see `callContext`), else
   * none. Outside any scope `observe_depth` is the deepest running
   * invocation's (`callDepth`), so the loop guard holds after an `await`.
   */
  private outgoingFields(request: TriggerRequest<unknown>): OutgoingFields {
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
    request: TriggerRequest<unknown>,
    namespace: string | undefined,
    activation?: ActivationWire,
  ): Promise<TOutput> {
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
          reject(timeoutError(effectiveTimeout, function_id))
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
   * The local path (sdk.md §4.3): runs the handler in-process, with a copy of
   * the payload, inside a new invocation context; the same timeout and the
   * same rejection value as the remote path; reports the call when an
   * observer matches. Puts nothing on the wire when nobody observes the
   * function.
   */
  private invokeLocal<TOutput>(
    function_id: string,
    handler: NonNullable<RemoteFunctionData['handler']>,
    observe: FunctionObservers,
    request: TriggerRequest<unknown>,
  ): Promise<TOutput> {
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
      metadata: request.metadata,
      localDepth: callLocalDepth() + 1,
    }
    const traceparent = request.traceparent ?? parent?.traceparent
    const baggage = request.baggage ?? parent?.baggage
    if (traceparent !== undefined) ctx.traceparent = traceparent
    if (baggage !== undefined) ctx.baggage = baggage

    // The observers this call is evaluated against: those active when it
    // started. None → it is not reported, and its input is not copied for it.
    const set = observe.delivery === 'auto' ? observe.compiled : []
    let input: unknown
    let observedInput: unknown
    let observedMetadata: unknown
    let metadata: unknown
    try {
      // What observers see is the payload and metadata as the caller passed them
      // when the call started, not what the handler or the caller made of them since.
      if (set.length > 0) {
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
        observe.reportLocal(set, {
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

    const effectiveTimeout = request.timeoutMs ?? this.invocationTimeoutMs
    const untrack = trackRunning(ctx, effectiveTimeout)
    let started: Promise<unknown>
    try {
      started = runInContext(ctx, () => handler(input, metadata))
    } catch (error) {
      // Thrown before the handler's body ran (a stack too deep to enter it).
      started = Promise.reject(error)
    }
    const done = started.then(
      (output) => {
        untrack()
        finish(true, output, undefined)
        return output
      },
      (error) => {
        untrack()
        const body = handlerErrorBody(error)
        finish(false, undefined, body)
        // What the remote path rejects with: the handler's error body, as the engine forwards it.
        throw body
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
            resolve(cloneValue(output) as TOutput)
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
        reject(timeoutError(effectiveTimeout, function_id))
      }, effectiveTimeout)
    })
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
    // this connection is replayed on the next one.
    this.registeredOnConnection = false

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
      const fallback = JSON.stringify({
        type: MessageType.InvocationResult,
        invocation_id: message.invocation_id,
        function_id: message.function_id,
        error: { code: error.code, message: payloadTooLargeMessage(size) },
      })
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

  private async onInvokeFunction<TInput>(
    invocation_id: string | undefined,
    function_id: string,
    input: TInput,
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

    const resolvedInput = this.resolveChannelValue(input) as TInput

    if (fn?.handler) {
      const handler = fn.handler
      // The invocation context every handler sees (`currentInvocation()`), and
      // what the calls it makes carry as their caller and trace context.
      const ctx: ActiveInvocation = {
        function_id,
        invocation_id: invocation_id ?? randomUUID(),
        observe_depth: observeFields.observe_depth ?? 0,
        metadata,
      }
      if (observeFields.caller) ctx.caller = observeFields.caller
      if (observeFields.activation) ctx.activation = observeFields.activation
      if (traceparent !== undefined) ctx.traceparent = traceparent
      if (baggage !== undefined) ctx.baggage = baggage
      // Running until it settles: what code after its `await`s falls back to (`callDepth`).
      const run = async () => {
        const untrack = trackRunning(ctx, MAX_RUNNING_INVOCATION_MS)
        try {
          return await runInContext(ctx, () => handler(resolvedInput, metadata))
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
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_id,
          result,
          traceparent,
          baggage,
        })
      } catch (error) {
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_id,
          error: handlerErrorBody(error),
          traceparent,
          baggage,
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
    namespace?: string
  }) {
    const { trigger_type, id, function_id, config, namespace } = message
    const triggerTypeData = this.triggerTypes.get(trigger_type)

    if (triggerTypeData) {
      try {
        await triggerTypeData.handler.registerTrigger({ id, function_id, config, namespace })
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
  }) {
    const trigger_type = message.trigger_type
    if (!trigger_type) return

    const triggerTypeData = this.triggerTypes.get(trigger_type)
    if (!triggerTypeData) return

    const { id, function_id = '', config } = message
    try {
      await triggerTypeData.handler.unregisterTrigger({ id, function_id, config })
    } catch (error) {
      console.error(`[iii] Error unregistering trigger ${id}`, error)
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
        console.warn(`[iii] Observer ${msg.id}: ${msg.warnings.join('; ')}`)
      }
      this.onObserverState(msg.id, msg.state, msg.error)
    } else if (msgType === MessageType.ObserverStateChanged) {
      const msg = decodeObserverStateChanged(message)
      this.onObserverState(msg.id, msg.state, msg.reason)
    } else if (msgType === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(message as { trigger_type: string; id: string; function_id: string; config: unknown })
    } else if (msgType === MessageType.UnregisterTrigger) {
      this.onUnregisterTrigger(
        message as { trigger_type?: string; id: string; function_id?: string; config?: unknown },
      )
    } else if (msgType === MessageType.RegistrationRejected) {
      this.onRegistrationRejected(
        message as { code: string; namespace: string; worker_name?: string; function_id?: string; owner_worker_id: string },
      )
    } else if (msgType === MessageType.WorkerRegistered) {
      const { worker_id, reattach_token, features } = decodeWorkerRegistered(message)
      this.workerId = worker_id
      this.reattachToken = reattach_token
      this.onEngineFeatures(features ?? [])
    }
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
      // What was queued while disconnected (an `unobserve`), then the replay.
      for (const frame of queued) this.sendMessageRaw(JSON.stringify(frame))
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

  /**
   * The engine rejected a registration. A `FUNCTION_NAMESPACE_CONFLICT` costs
   * one function and leaves the connection open, so log and keep serving. A
   * `WORKER_NAMESPACE_CONFLICT` means another live worker owns this name in the
   * namespace: it is terminal — stop and do not reconnect (otherwise the engine
   * would only reject the same name again, which with `maxRetries: -1` loops
   * forever).
   */
  private onRegistrationRejected(init: {
    code: string
    namespace: string
    worker_name?: string
    function_id?: string
    owner_worker_id: string
  }): void {
    if (init.code === 'FUNCTION_NAMESPACE_CONFLICT') {
      // The engine refused it: no local dispatch, no observers for it here.
      if (init.function_id !== undefined)
        this.functions.get(init.function_id)?.observe?.onRegistrationRejected(this.connectionEpoch)
      console.warn(
        `[iii] Function registration rejected: function "${init.function_id ?? '<unknown>'}" in namespace "${init.namespace}" is already exported by worker ${init.owner_worker_id}. Staying connected and serving the rest.`,
      )
      return
    }

    console.error(
      `[iii] Registration rejected (${init.code}): worker "${init.worker_name}" in namespace "${init.namespace}" is already owned by ${init.owner_worker_id}. Not reconnecting.`,
    )
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
}

/**
 * Factory object that constructs routing actions for {@link ISdk.trigger}.
 *
 * @example
 * ```typescript
 * import { TriggerAction } from 'iii-browser-sdk'
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
