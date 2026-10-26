export enum MessageType {
  RegisterFunction = 'registerfunction',
  UnregisterFunction = 'unregisterfunction',
  InvokeFunction = 'invokefunction',
  InvocationResult = 'invocationresult',
  RegisterTriggerType = 'registertriggertype',
  RegisterTrigger = 'registertrigger',
  UnregisterTrigger = 'unregistertrigger',
  UnregisterTriggerType = 'unregistertriggertype',
  TriggerRegistrationResult = 'triggerregistrationresult',
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
  /** Engine → owner: the `registerfunction` ack, with the function's full observer set. */
  FunctionRegistered = 'functionregistered',
  /** Engine → owner: one observer attached to a function this connection hosts. */
  ObserverAdded = 'observeradded',
  /** Engine → owner: one observer detached. */
  ObserverRemoved = 'observerremoved',
  /** Owner → engine: accept (or reject) an added observer. */
  ObserverAck = 'observerack',
  /** Owner → engine: matched local invocations and `obs.emit` events, batched. */
  InvocationsObserved = 'invocationsobserved',
  /** Owner → engine: periodic counts of local invocations and report drops. */
  LocalInvocationStats = 'localinvocationstats',
}

/**
 * Protocol features (`WorkerRegistered.features` from the engine,
 * `engine::workers::register` `features` from the SDK).
 */
export const FEATURE_OBSERVE_V1 = 'observe.v1'
export const FEATURE_LOCAL_DISPATCH_V1 = 'local-dispatch.v1'
/**
 * What this SDK declares in `engine::workers::register`. Not
 * `large-messages.v1`, unlike the other SDKs: a browser receives one 16 MiB
 * WebSocket frame at most.
 */
export const SDK_FEATURES: readonly string[] = [FEATURE_OBSERVE_V1, FEATURE_LOCAL_DISPATCH_V1]

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
/** When an auto observer fires (`on`). */
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

/** `RegisterFunction.observe`: the part of the observe hooks that travels. Callbacks never do. */
export type ObserveDescriptorWire = {
  /** `auto` (default) or `owner`. */
  delivery?: OpenEnum<'auto' | 'owner'>
  /** Owner only: validates the observers' `args` in the engine (`INVALID_ARGS`). */
  args_format?: RegisterFunctionFormat
  /** Phase 5 shim: accept old `registertrigger { trigger_type }` for this type. */
  legacy_trigger_type?: string
  /** Default `true`; `false` makes every `observe` on it fail with `NOT_OBSERVABLE`. */
  allow?: boolean
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
 * An observer as its function's owner sees it: never the observer's metadata,
 * policy or connection. `target` is the target function id.
 */
export type ObserverInfoWire = {
  id: string
  args: Record<string, unknown>
  /** The observer's filters, normalized by the engine. `{}` matches every event. */
  filters: Record<string, unknown>
  target: string
  capture: CaptureWire[]
  on: ObserveOnWire
  /** Phase 5 shim only: the old trigger config the owner reads as args. */
  legacy?: { trigger_type: string; config: unknown }
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

/** One matched local invocation (auto) or one `obs.emit` event (owner). */
export type ObservedItemWire = {
  function_id: string
  namespace?: string
  invocation_id: string
  /** Observer ids whose filters matched (the same event coalesced for several). */
  matched: string[]
  status: OpenEnum<'success' | 'error'>
  /** `null` is a value here: absent means not captured or does not exist. */
  input?: unknown
  output?: unknown
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
  /** `BUFFER_FULL` | `TOO_LARGE` (SDK-side drops). */
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
  /** Protocol features the engine supports (`observe.v1`, `local-dispatch.v1`); absent on engines older than 0.25. */
  features?: string[]
}

/** @deprecated Trigger types are replaced by functions with an observe descriptor (`RegisterFunctionMessage.observe`). */
export type RegisterTriggerTypeMessage = {
  message_type: MessageType.RegisterTriggerType
  /** Unique identifier for the trigger type (e.g. `state`, `durable:subscriber`). */
  id: string
  /** Human-readable description of what this trigger type does. */
  description: string
}

/** @deprecated Trigger types are replaced by functions with an observe descriptor. */
export type UnregisterTriggerTypeMessage = {
  message_type: MessageType.UnregisterTriggerType
  id: string
  namespace?: string
}

export type UnregisterTriggerMessage = {
  message_type: MessageType.UnregisterTrigger
  id: string
  type?: string
}

export type TriggerRegistrationResultMessage = {
  message_type: MessageType.TriggerRegistrationResult
  id: string
  type: string
  function_id: string
  result?: unknown
  error?: unknown
}

/**
 * The engine refused a registration. `FUNCTION_NAMESPACE_CONFLICT` costs one
 * function and leaves the connection open; `WORKER_NAMESPACE_CONFLICT` is
 * terminal — another live worker owns this name in the namespace.
 */
export type RegistrationRejectedMessage = {
  message_type: MessageType.RegistrationRejected
  /** Rejection code: `FUNCTION_NAMESPACE_CONFLICT` or `WORKER_NAMESPACE_CONFLICT`. */
  code: string
  /** Namespace the conflict occurred in. */
  namespace: string
  /** Contested worker name. Present only for `WORKER_NAMESPACE_CONFLICT`. */
  worker_name?: string
  /** Contested function id. Present only for `FUNCTION_NAMESPACE_CONFLICT`. */
  function_id?: string
  /** ID of the worker that already owns the name. */
  owner_worker_id: string
}

/** @deprecated Replaced by the `observe` message (`iii.observe`). */
export type RegisterTriggerMessage = {
  /** Wire discriminator; always `MessageType.RegisterTrigger`. */
  message_type: MessageType.RegisterTrigger
  /** Unique trigger identifier, generated by the SDK during registration. */
  id: string
  /** Identifier of the registered trigger type this trigger uses (e.g. `storage::object-created`, `http`). */
  type: string
  /** ID of the function this trigger invokes when it fires. */
  function_id: string
  /** Trigger-type-specific configuration, matching the shape the trigger type expects. */
  config: unknown
  /**
   * Namespace the trigger's target function resolves in.
   *
   * Omitting it does not bind in the engine's default: `registerTrigger` fills
   * it from this worker's namespace, because the function a trigger names is
   * one this worker registered, and that landed in the worker's namespace. Name
   * another namespace, `default` included, to bind elsewhere.
   */
  namespace?: string
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
   * The description of the function
   */
  description?: string
  /**
   * The request format of the function
   */
  request_format?: RegisterFunctionFormat
  /**
   * The response format of the function
   */
  response_format?: RegisterFunctionFormat
  metadata?: Record<string, unknown>
  /**
   * The observe descriptor: how observers of this function are delivered.
   * Only these four fields travel; the hooks stay in the SDK.
   */
  observe?: ObserveDescriptorWire
}

/**
 * Routing action for {@link TriggerRequest}. Determines how the engine
 * handles the invocation.
 *
 * - `enqueue`: Routes through a named queue for async processing.
 * - `void`: Fire-and-forget, no response.
 */
export type TriggerAction = { type: 'enqueue'; queue: string } | { type: 'void' }

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
  /** Trigger type IDs the worker may register triggers for. When omitted, all types are allowed. */
  allowed_trigger_types?: string[]
  /** Whether the worker may register new trigger types. Defaults to `false` if omitted. */
  allow_trigger_type_registration?: boolean
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
  action?: TriggerAction
  /** Auth context returned by the auth function for this session. */
  context: Record<string, unknown>
  /**
   * Target namespace the invoke addressed; forward the call here to stay in the
   * caller's namespace. Absent → the engine's default namespace.
   */
  namespace?: string
}

/**
 * Input passed to the `on_trigger_type_registration_function_id` hook
 * when a worker attempts to register a new trigger type through the RBAC port.
 * Return an {@link OnTriggerTypeRegistrationResult} with the (possibly mapped)
 * fields, or throw to deny the registration.
 */
export type OnTriggerTypeRegistrationInput = {
  /** ID of the trigger type being registered. */
  trigger_type_id: string
  /** Human-readable description of the trigger type. */
  description: string
  /** Auth context from `AuthResult.context` for this session. */
  context: Record<string, unknown>
}

/**
 * Result returned from the `on_trigger_type_registration_function_id` hook.
 * All fields are optional; omitted fields keep the original value from the
 * registration request.
 */
export type OnTriggerTypeRegistrationResult = {
  /** Mapped trigger type ID. */
  trigger_type_id?: string
  /** Mapped description. */
  description?: string
}

/**
 * Input passed to the `on_trigger_registration_function_id` hook
 * when a worker attempts to register a trigger through the RBAC port.
 * Return an {@link OnTriggerRegistrationResult} with the (possibly mapped)
 * fields, or throw to deny the registration.
 */
export type OnTriggerRegistrationInput = {
  /** ID of the trigger being registered. */
  trigger_id: string
  /** Trigger type identifier. */
  trigger_type: string
  /** ID of the function this trigger is bound to. */
  function_id: string
  /** Trigger-specific configuration. */
  config: unknown
  /**
   * Namespace the trigger's target resolves in (explicit, or `default` when
   * absent). The same id can exist in several namespaces, so the hook needs this
   * to authorize per target namespace.
   */
  namespace?: string
  /** Auth context from `AuthResult.context` for this session. */
  context: Record<string, unknown>
}

/**
 * Result returned from the `on_trigger_registration_function_id` hook.
 * All fields are optional; omitted fields keep the original value from the
 * registration request.
 */
export type OnTriggerRegistrationResult = {
  /** Mapped trigger ID. */
  trigger_id?: string
  /** Mapped trigger type. */
  trigger_type?: string
  /** Mapped function ID. */
  function_id?: string
  /** Mapped trigger configuration. */
  config?: unknown
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
}

/**
 * Result returned when a function is invoked with `TriggerAction.Enqueue`.
 */
export type EnqueueResult = {
  /** Unique receipt ID for the enqueued message. */
  messageReceiptId: string
}

/**
 * Request object passed to {@link ISdk.trigger}.
 *
 * @typeParam TInput - Type of the payload.
 */
export type TriggerRequest<TInput = unknown> = {
  /** ID of the function to invoke. */
  function_id: string
  /** Input data passed to the function. */
  payload: TInput
  /** Sets how the trigger is routed. Omit for a synchronous request/response. Specify for a specific routing scheme (e.g. `TriggerAction.Enqueue()`, `TriggerAction.Void()`). */
  action?: TriggerAction
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
   * Trigger action for queue routing or fire-and-forget
   */
  action?: TriggerAction
  /**
   * Namespace to route this invocation to. The engine reads an absent field as
   * its default namespace; a call made through this SDK does not arrive absent,
   * because `trigger` fills it from this worker's namespace.
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
  /** Present on every activation of an observer's target; marks an owner delivery (`obs.call`) outbound. */
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
  | RegisterTriggerMessage
  | RegisterTriggerTypeMessage
  | UnregisterTriggerMessage
  | UnregisterTriggerTypeMessage
  | TriggerRegistrationResultMessage
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
