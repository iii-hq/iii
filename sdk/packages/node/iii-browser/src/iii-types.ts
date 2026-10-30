import type { Observer } from './observe-types'

export enum MessageType {
  RegisterFunction = 'registerfunction',
  UnregisterFunction = 'unregisterfunction',
  InvokeFunction = 'invokefunction',
  InvocationResult = 'invocationresult',
  RegistrationRejected = 'registrationrejected',
  WorkerRegistered = 'workerregistered',
  Reattach = 'reattach',
  /** Observing worker → engine: create (or idempotently replay) an observer. */
  Observe = 'observe',
  /** Observing worker → engine: dispose an observer this connection owns. */
  Unobserve = 'unobserve',
  /** Engine → observing worker: the reply to `observe` (also on replay). */
  ObserveResult = 'observeresult',
  /** Engine → observing worker: an observer's lifecycle transition. */
  ObserverStateChanged = 'observerstatechanged',
  /** Engine → host: the `registerfunction` ack, with the function's full observer set. */
  FunctionRegistered = 'functionregistered',
  /** Engine → host: one observer attached to a function this connection hosts. */
  ObserverAdded = 'observeradded',
  /** Engine → host: one observer detached. */
  ObserverRemoved = 'observerremoved',
  /** Host → engine: accept (or reject) an added observer. */
  ObserverAck = 'observerack',
  /** Host → engine: the calls of its functions that observers matched, batched. */
  InvocationsObserved = 'invocationsobserved',
  /** Host → engine: periodic counts of local invocations and report drops. */
  LocalInvocationStats = 'localinvocationstats',
}

/**
 * Protocol features (`WorkerRegistered.features` from the engine,
 * `engine::workers::register` `features` from the SDK).
 */
export const FEATURE_OBSERVE_V1 = 'observe.v1'
/**
 * The host delivers the calls of its functions (routed ones included), and
 * functions may set `args_format`, `private`, `observable: false` and
 * `onObserverDeliver`. Includes `observe.v1`.
 */
export const FEATURE_OBSERVE_V2 = 'observe.v2'
export const FEATURE_LOCAL_DISPATCH_V1 = 'local-dispatch.v1'
/**
 * What this SDK declares in `engine::workers::register`. Not
 * `large-messages.v1`, unlike the other SDKs: a browser receives one 16 MiB
 * WebSocket frame at most.
 */
export const SDK_FEATURES: readonly string[] = [FEATURE_OBSERVE_V1, FEATURE_OBSERVE_V2, FEATURE_LOCAL_DISPATCH_V1]

/** The error body of an `InvocationResult` (and of observe errors and state reasons). */
export type ErrorBody = {
  code: string
  message: string
  stacktrace?: string
}

/**
 * A wire enum that a newer peer may extend: decoders keep a value they do not
 * know instead of failing the frame.
 */
export type OpenEnum<T extends string> = T | (string & {})

/** What an observer ships to its target (`capture`). */
export type CaptureWire = OpenEnum<'input' | 'output' | 'error' | 'caller' | 'metadata'>
/** Which outcomes of a call reach the observer (`on`). */
export type ObserveOnWire = OpenEnum<'success' | 'error' | 'complete'>
/** What the target receives (`deliver`). */
export type ObserveDeliverWire = OpenEnum<'envelope' | 'input' | 'output'>
/** Observer lifecycle state. */
export type ObserverStateWire = OpenEnum<'pending' | 'active' | 'suspended' | 'error' | 'disposed'>

/**
 * Who started an invocation. Engine → worker: `worker_id` / `worker_name`
 * are stamped by the engine from the calling connection. Worker → engine:
 * the SDK sends `function_id`, `invocation_id` and `parent` (the caller of the
 * handler the call was made from, one level only).
 */
export type CallerWire = {
  worker_id?: string
  worker_name?: string
  function_id?: string
  invocation_id?: string
  parent?: Omit<CallerWire, 'parent'>
}

/** `InvokeFunction.activation`: present on every activation of an observer's target. */
export type ActivationWire = {
  observer_id: string
  activation_id?: string
}

/** `observe.source`: the left side of `iii.observe`. */
export type ObserveSourceWire = {
  function_id: string
  namespace?: string
  args?: Record<string, unknown>
  filters?: Record<string, unknown>
  capture?: CaptureWire[]
  on?: ObserveOnWire
  deliver?: ObserveDeliverWire
}

export type ObserveMessage = {
  message_type: MessageType.Observe
  /** Client-generated UUID, stable across replays. */
  id: string
  source: ObserveSourceWire
  target: { function_id: string; namespace?: string }
  metadata?: Record<string, unknown>
  max_fires?: number
  expires_in_ms?: number
  max_in_flight?: number
}

export type UnobserveMessage = {
  message_type: MessageType.Unobserve
  id: string
}

export type ObserveResultMessage = {
  message_type: MessageType.ObserveResult
  id: string
  state: ObserverStateWire
  error?: ErrorBody
  warnings?: string[]
}

export type ObserverStateChangedMessage = {
  message_type: MessageType.ObserverStateChanged
  id: string
  state: ObserverStateWire
  reason?: ErrorBody
  fires: number
}

/**
 * An observer as the host of its function sees it: never the observer's
 * metadata, policy or connection. `target` is the target function id.
 */
export type ObserverInfoWire = {
  id: string
  args: Record<string, unknown>
  /** The observer's filters, normalized by the engine. `{}` matches every call. */
  filters: Record<string, unknown>
  target: string
  capture: CaptureWire[]
  on: ObserveOnWire
}

export type FunctionRegisteredMessage = {
  message_type: MessageType.FunctionRegistered
  id: string
  namespace: string
  revision: number
  local_dispatch: boolean
  observers: ObserverInfoWire[]
}

export type ObserverAddedMessage = {
  message_type: MessageType.ObserverAdded
  function_id: string
  namespace: string
  revision: number
  observer: ObserverInfoWire
}

export type ObserverRemovedMessage = {
  message_type: MessageType.ObserverRemoved
  function_id: string
  namespace: string
  revision: number
  id: string
}

export type ObserverAckMessage = {
  message_type: MessageType.ObserverAck
  function_id: string
  namespace?: string
  id: string
  revision: number
  /** Rejects the observer (also a late rejection of an active one). */
  error?: ErrorBody
}

/** One local call of a hosted function, with the observers its delivery selected. */
export type ObservedItemWire = {
  function_id: string
  namespace?: string
  invocation_id: string
  /** The observers delivered. */
  matched: string[]
  status: OpenEnum<'success' | 'error'>
  /** `null` is a value here: absent means not captured or does not exist. */
  input?: unknown
  /** Absent with `output_is_input`: the engine fills it from `input`. */
  output?: unknown
  /** The handler returned its input; left out when `false`. */
  output_is_input?: boolean
  error?: unknown
  caller?: CallerWire
  metadata?: unknown
  started_at_ms: number
  duration_ms: number
  observe_depth?: number
  traceparent?: string
  baggage?: string
}

export type InvocationsObservedMessage = {
  message_type: MessageType.InvocationsObserved
  items: ObservedItemWire[]
}

export type LocalFunctionStatsWire = {
  function_id: string
  count: number
  errors?: number
  duration_ms_sum?: number
}

export type DroppedReportStatsWire = {
  function_id: string
  /** `BUFFER_FULL` | `TOO_LARGE` | `INACTIVE` (SDK-side drops). */
  reason: string
  count: number
}

export type LocalInvocationStatsMessage = {
  message_type: MessageType.LocalInvocationStats
  window_ms: number
  functions: LocalFunctionStatsWire[]
  dropped_reports: DroppedReportStatsWire[]
}

export type WorkerRegisteredMessage = {
  message_type: MessageType.WorkerRegistered
  worker_id: string
  /** Secret to present in a reattach frame on reconnect; absent on older engines. */
  reattach_token?: string
  /** Protocol features the engine supports (`observe.v1`, `observe.v2`, `local-dispatch.v1`); absent on engines older than 0.25. */
  features?: string[]
}

/**
 * The engine refused a registration. `WORKER_NAMESPACE_CONFLICT` is terminal:
 * another live worker owns this name in the namespace. The others cost one
 * function and leave the connection open: `FUNCTION_NAMESPACE_CONFLICT`
 * (another worker hosts the id), `INVALID_ARGS_FORMAT` (its `args_format` is
 * not a valid JSON Schema) and `UNSUPPORTED`.
 */
export type RegistrationRejectedMessage = {
  message_type: MessageType.RegistrationRejected
  /** Machine-readable rejection code. */
  code: string
  /** Namespace of the refused registration. */
  namespace: string
  /** Contested worker name. Present only for `WORKER_NAMESPACE_CONFLICT`. */
  worker_name?: string
  /** The refused function. Present on every rejection of one function. */
  function_id?: string
  /** ID of the worker that already owns the contested identity; `""` on a rejection that is not a conflict. */
  owner_worker_id: string
}

export type RegisterFunctionFormat = {
  /**
   * The name of the parameter
   */
  name?: string
  /**
   * The description of the parameter
   */
  description?: string
  /**
   * The type of the parameter
   */
  type?: 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map' | 'integer'
  /**
   * The body of the parameter (for objects)
   */
  properties?: Record<string, unknown>
  /**
   * The items of the parameter (for arrays)
   */
  items?: unknown
  /**
   * Whether the parameter is required
   */
  required?: string[]
  [key: string]: unknown
}

export type RegisterFunctionMessage = {
  message_type: MessageType.RegisterFunction
  /**
   * The path of the function (use :: for namespacing, e.g. external::my_lambda)
   */
  id: string
  /**
   * Human-readable summary of the function, listed by discovery
   * (`engine::functions::list`).
   */
  description?: string
  /**
   * JSON Schema of the call's input.
   */
  request_format?: RegisterFunctionFormat
  /**
   * JSON Schema of the handler's return value: what the function's observers
   * observe.
   */
  response_format?: RegisterFunctionFormat
  /**
   * Arbitrary metadata attached to the function, stored with it and listed by
   * discovery (`engine::functions::list`). It is distinct from the per-call
   * metadata a handler receives.
   */
  metadata?: Record<string, unknown>
  /**
   * JSON Schema of the `args` an observer of this function passes. The
   * engine validates every observer's args against it (`INVALID_ARGS`)
   * before the added hooks run, and refuses a schema that is not valid
   * (`INVALID_ARGS_FORMAT`). A function without it takes no args.
   */
  args_format?: RegisterFunctionFormat
  /**
   * Only this worker may call the function: the engine answers a call from
   * any other connection with `FORBIDDEN`, and refuses the function as an
   * observer's target. Any worker may observe it, and its observers receive
   * every call this worker makes. For event sources, whose calls from
   * anywhere else would be forged events. This worker's own calls pass the
   * input and the result by reference: the handler and hooks treat both as
   * read-only. Default `false`; sent only when `true`.
   */
  private?: boolean
  /**
   * `false`: nobody may observe the function (`NOT_OBSERVABLE`), for one
   * whose payloads carry credentials. Default `true`; sent only when `false`.
   */
  observable?: boolean
}

/**
 * Routing action for {@link CallRequest}. Determines how the engine
 * handles the invocation.
 *
 * - `enqueue`: Routes through a named queue for async processing.
 * - `void`: Fire-and-forget, no response.
 */
export type CallAction = { type: 'enqueue'; queue: string } | { type: 'void' }

/** @deprecated Use {@link CallAction}, the same type. */
export type TriggerAction = CallAction

/**
 * Input passed to the RBAC auth function during WebSocket upgrade.
 * Contains the HTTP headers, query parameters, and client IP from the
 * connecting worker's upgrade request.
 */
export type AuthInput = {
  /** HTTP headers from the WebSocket upgrade request. */
  headers: Record<string, string>
  /** Query parameters from the upgrade URL. Each key maps to an array of values to support repeated keys. */
  query_params: Record<string, string[]>
  /** IP address of the connecting client. */
  ip_address: string
}

/**
 * Return value from the RBAC auth function. Controls which functions the
 * authenticated worker can invoke and what context is forwarded to the
 * middleware.
 */
export type AuthResult = {
  /**
   * Grants scoped to one namespace each: `{ orders: ['svc::*'] }`. A value is an
   * exact function ID or a wildcard, in either the bare (`svc::*`) or the
   * `match("svc::*")` spelling.
   *
   * The keys are also the namespaces the session may declare on
   * `engine::workers::register`. Omit to scope nothing: the session declares any
   * namespace, and only `allowed_functions` and `expose_functions` apply.
   */
  namespaces?: Record<string, string[]>
  /** Additional function IDs to allow beyond the `expose_functions` config, in the `default` namespace only. Put per-namespace grants in `namespaces`. Defaults to `[]` if omitted. */
  allowed_functions?: string[]
  /** Function IDs to deny even if they match `expose_functions`. Takes precedence over allowed. Defaults to `[]` if omitted. */
  forbidden_functions?: string[]
  /** Whether the worker may register new functions. Defaults to `true` if omitted. */
  allow_function_registration?: boolean
  /** Arbitrary context forwarded to the middleware function on every invocation. Defaults to `{}` if omitted. */
  context?: Record<string, unknown>
  /** Optional prefix applied to all function IDs registered by this worker. */
  function_registration_prefix?: string
}

/**
 * Input passed to the RBAC middleware function on every function invocation
 * through the RBAC port. The middleware can inspect, modify, or reject the
 * call before it reaches the target function.
 */
export type MiddlewareFunctionInput = {
  /** ID of the function being invoked. */
  function_id: string
  /** Payload sent by the caller. */
  payload: Record<string, unknown>
  /** Routing action, if any. */
  action?: CallAction
  /** Auth context returned by the auth function for this session. */
  context: Record<string, unknown>
  /**
   * Target namespace the invoke addressed; forward the call here to stay in the
   * caller's namespace. Absent → the engine's default namespace.
   */
  namespace?: string
}

/**
 * Input passed to the `on_function_registration_function_id` hook
 * when a worker attempts to register a function through the RBAC port.
 * Return an {@link OnFunctionRegistrationResult} with the (possibly mapped)
 * fields, or throw to deny the registration.
 */
export type OnFunctionRegistrationInput = {
  /** ID of the function being registered. */
  function_id: string
  /** Human-readable description of the function. */
  description?: string
  /** Arbitrary metadata attached to the function. */
  metadata?: Record<string, unknown>
  /**
   * Namespace the function registers in. The same id can exist in several
   * namespaces, so the hook needs this to authorize per namespace.
   */
  namespace?: string
  /** Only the registering worker may call the function; always sent. */
  private: boolean
  /** `false`: nobody may observe the function; always sent. */
  observable: boolean
  /** JSON Schema of the args its observers pass; sent when the registration sets it. */
  args_format?: Record<string, unknown>
  /** Auth context from `AuthResult.context` for this session. */
  context: Record<string, unknown>
}

/**
 * Result returned from the `on_function_registration_function_id` hook.
 * All fields are optional; omitted fields keep the original value from the
 * registration request.
 */
export type OnFunctionRegistrationResult = {
  /** Mapped function ID. */
  function_id?: string
  /** Mapped description. */
  description?: string
  /** Mapped metadata. */
  metadata?: Record<string, unknown>
  /** Replaces the registration's `private`. */
  private?: boolean
  /** Replaces the registration's `observable`. */
  observable?: boolean
  /** Replaces the registration's `args_format`. */
  args_format?: Record<string, unknown>
}

/**
 * Result returned when a function is called with `CallAction.Enqueue`.
 */
export type EnqueueResult = {
  /** Unique receipt ID for the enqueued message. */
  messageReceiptId: string
}

/**
 * Request object passed to {@link ISdk.call}: a function by id, or the target
 * of an observer.
 *
 * @typeParam TInput - Type of the payload.
 */
export type CallRequest<TInput = unknown> =
  | {
      /** ID of the function to invoke. */
      function_id: string
      observer?: never
      /** Input data passed to the function. */
      payload: TInput
      /** Sets how the call is routed. Omit for a synchronous request/response. Specify for a specific routing scheme (e.g. `CallAction.Enqueue()`, `CallAction.Void()`). */
      action?: CallAction
      /** Override the default invocation timeout, in milliseconds. */
      timeoutMs?: number
      /** Namespace to route this invocation to. Omit to inherit this worker's; say `default` to reach the engine's from a namespaced worker. */
      namespace?: string
      /**
       * Per-invocation metadata (any JSON). Travels beside the payload: the
       * handler receives it as its second argument, and observer filters read it
       * as `metadata.*`.
       */
      metadata?: unknown
      /**
       * W3C `traceparent` to continue. The browser SDK has no OpenTelemetry, so it
       * never makes one up: when omitted, a call made inside a handler forwards
       * the `traceparent` that handler was invoked with (see
       * {@link ISdk.currentInvocation}); a top-level call sends none and the engine
       * starts the trace.
       */
      traceparent?: string
      /** W3C `baggage` to send along with `traceparent`; defaults the same way. */
      baggage?: string
      /**
       * `auto` (default) runs a function this worker hosts in-process when the
       * engine confirmed it for local dispatch; `remote` always goes through the
       * engine. Tests use it, and so do callers that rely on engine-side effects.
       */
      dispatch?: 'auto' | 'remote'
    }
  | {
      /**
       * An observer of a function this worker hosts: the call runs the
       * observer's target, with the observer's policy, metadata and loop
       * guard. Only the host of the observed function may call through it
       * (`NOT_FUNCTION_OWNER`).
       */
      observer: Observer
      function_id?: never
      /** Input data passed to the target, as given (the observer's filters are not applied). */
      payload: TInput
      /** `CallAction.Void()` for fire-and-forget. Omit for a synchronous request/response. */
      action?: CallAction
      /** Override the default invocation timeout, in milliseconds. */
      timeoutMs?: number
    }

/** @deprecated Use {@link CallRequest}, the same type. */
export type TriggerRequest<TInput = unknown> = CallRequest<TInput>

export type InvokeFunctionMessage = {
  message_type: MessageType.InvokeFunction
  /**
   * This is optional for async invocations
   */
  invocation_id?: string
  /**
   * The path of the function
   */
  function_id: string
  /**
   * The data to pass to the function
   */
  data: unknown
  /**
   * W3C trace-context traceparent header for distributed tracing
   */
  traceparent?: string
  /**
   * W3C baggage header for cross-cutting context propagation
   */
  baggage?: string
  /**
   * Call action for queue routing or fire-and-forget
   */
  action?: CallAction
  /**
   * Namespace to route this invocation to. The engine reads an absent field as
   * its default namespace; a call made through this SDK does not arrive absent,
   * because `call` fills it from this worker's namespace.
   */
  namespace?: string
  /** Per-invocation metadata (any JSON); absent means none. */
  metadata?: unknown
  /**
   * Who made the call. Sent by the SDK when the call is made inside a handler;
   * on inbound calls `worker_id` / `worker_name` are stamped by the engine.
   */
  caller?: CallerWire
  /** Depth of the causal chain in observe hops; omitted when 0. */
  observe_depth?: number
  /** Present on every activation of an observer's target; outbound, marks a call through an observer. */
  activation?: ActivationWire
}

export type InvocationResultMessage = {
  message_type: MessageType.InvocationResult
  /**
   * The id of the invocation
   */
  invocation_id: string
  /**
   * The path of the function
   */
  function_id: string
  result?: unknown
  error?: unknown
  /**
   * W3C trace-context traceparent header for distributed tracing
   */
  traceparent?: string
  /**
   * W3C baggage header for cross-cutting context propagation
   */
  baggage?: string
  /**
   * Host → engine (`observe.v2`): the observers iii's delivery selected for
   * this routed call. Left out when empty, and by a function with
   * `onObserverDeliver`.
   */
  observed?: string[]
}

export type UnregisterFunctionMessage = {
  message_type: MessageType.UnregisterFunction
  id: string
}

/**
 * Serializable reference to one end of a streaming channel. Can be included
 * in invocation payloads to pass channel endpoints between workers.
 */
export type StreamChannelRef = {
  /** Unique channel identifier. */
  channel_id: string
  /** Access key for authentication. */
  access_key: string
  /** Whether this ref is for reading or writing. */
  direction: 'read' | 'write'
}

export type IIIMessage =
  | RegisterFunctionMessage
  | UnregisterFunctionMessage
  | InvokeFunctionMessage
  | InvocationResultMessage
  | RegistrationRejectedMessage
  | WorkerRegisteredMessage
  | ObserveMessage
  | UnobserveMessage
  | ObserveResultMessage
  | ObserverStateChangedMessage
  | FunctionRegisteredMessage
  | ObserverAddedMessage
  | ObserverRemovedMessage
  | ObserverAckMessage
  | InvocationsObservedMessage
  | LocalInvocationStatsMessage
