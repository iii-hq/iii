import type { HttpInvocationConfig } from '@iii-dev/helpers/http'

/**
 * Any JSON value: the TypeScript equivalent of the engine's arbitrary-JSON
 * wire values (Rust `serde_json::Value`). Used where the wire contract is
 * "any JSON", e.g. per-invocation `metadata`.
 */
export type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue }

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
  WorkerRegistered = 'workerregistered',
  RegistrationRejected = 'registrationrejected',
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
/** What this SDK declares in `engine::workers::register`. */
export const SDK_FEATURES: readonly string[] = [FEATURE_OBSERVE_V1, FEATURE_LOCAL_DISPATCH_V1]

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

/** @deprecated Trigger types are replaced by functions with an observe descriptor (`RegisterFunctionMessage.observe`). */
export type RegisterTriggerTypeMessage = {
  message_type: MessageType.RegisterTriggerType
  /** Unique identifier for the trigger type (e.g. `state`, `durable:subscriber`). */
  id: string
  /** Human-readable description of what this trigger type does. */
  description: string
  /**
   * Namespace this provider serves. Omit to let the engine use this
   * connection's own, which is what a worker providing a trigger type for its
   * own project wants.
   */
  namespace?: string
}

export type UnregisterTriggerTypeMessage = {
  message_type: MessageType.UnregisterTriggerType
  id: string
}

export type UnregisterTriggerMessage = {
  message_type: MessageType.UnregisterTrigger
  id: string
  type?: string
}

export type ErrorBody = {
  code: string
  message: string
  stacktrace?: string
}

export type TriggerRegistrationResultMessage = {
  message_type: MessageType.TriggerRegistrationResult
  id: string
  type: string
  function_id: string
  error?: ErrorBody
}

/** @deprecated Replaced by {@link ObserveMessage} (`iii.observe`). */
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
  /** Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation. */
  metadata?: Record<string, unknown>
  /**
   * Namespace the trigger's target function resolves in.
   *
   * Omitting it does not bind in the engine's default: `registerTrigger` fills
   * it from this worker's namespace, because the function a trigger names is
   * one this worker registered, and that landed in the worker's namespace. Name
   * another namespace, `default` included, to bind elsewhere.
   */
  namespace?: string
  /**
   * Namespace to find the trigger type's provider in.
   *
   * Omitting it is not the same as `'default'`: it asks the engine to resolve,
   * taking this worker's namespace first and the engine's own second. That is
   * what lets a project ship its own provider for a type id the engine also
   * provides, while a worker that has not been migrated keeps reaching the
   * engine's without saying so. Naming one is strict.
   */
  trigger_namespace?: string
}

export type RegisterFunctionFormat = {
  /**
   * The name of the parameter.
   */
  name?: string
  /**
   * The description of the parameter.
   */
  description?: string
  /**
   * The type of the parameter.
   */
  type?: 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map' | 'integer'
  /**
   * The body of the parameter (for objects).
   */
  properties?: Record<string, unknown>
  /**
   * The items of the parameter (for arrays).
   */
  items?: unknown
  /**
   * Whether the parameter is required.
   */
  required?: string[]
  [key: string]: unknown
}

export type RegisterFunctionMessage = {
  message_type: MessageType.RegisterFunction
  /**
   * The path of the function (use :: for namespacing, e.g. external::my_lambda).
   */
  id: string
  /**
   * The description of the function.
   */
  description?: string
  /**
   * The request format of the function.
   */
  request_format?: RegisterFunctionFormat
  /**
   * The response format of the function.
   */
  response_format?: RegisterFunctionFormat
  /**
   * Arbitrary metadata attached to the function.
   */
  metadata?: Record<string, unknown>
  /**
   * HTTP invocation config for external HTTP functions (Lambda, Cloudflare Workers, etc.).
   */
  invocation?: HttpInvocationConfig
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
 * Request object passed to {@link IIIClient.trigger}.
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
  /** Arbitrary user-specifiable metadata supplied to the triggered handler function on every invocation. */
  metadata?: unknown
  /**
   * Target namespace for routing. Omit to inherit this worker's; say `default`
   * to reach the engine's from a namespaced worker. Serialized into the
   * {@link InvokeFunctionMessage} `namespace` field.
   */
  namespace?: string
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
   * The path of the function.
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
   * Optional per-invocation metadata (arbitrary JSON). Travels as a separate
   * channel from `data`. Omitted from the JSON when undefined; absence on
   * inbound means "no metadata" (backward compatible with older engines).
   */
  metadata?: JsonValue
  /**
   * Target namespace for routing. Optional and additive: omitted from the JSON
   * when undefined, and the engine reads an absent field as its default
   * namespace (backward compatible with older engines).
   *
   * A call made through this SDK does not arrive absent: `trigger` fills it
   * from this worker's namespace unless the request names one.
   */
  namespace?: string
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
   * The path of the function.
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

export type WorkerRegisteredMessage = {
  message_type: MessageType.WorkerRegistered
  worker_id: string
  /** Secret to present in a reattach frame on reconnect; absent on older engines. */
  reattach_token?: string
  /** Protocol features the engine supports (`observe.v1`, `local-dispatch.v1`); absent on engines older than 0.25. */
  features?: string[]
}

/**
 * Sent by the engine when this worker's registration collides with a live
 * worker in `namespace`. The `code` decides severity:
 *
 * - `WORKER_NAMESPACE_CONFLICT`: another worker already holds this
 *   `(namespace, worker_name)`. The engine closes the connection -- fatal: the
 *   SDK stops the worker and does not reconnect.
 * - `FUNCTION_NAMESPACE_CONFLICT`: another worker already exports this one
 *   `function_id`. The engine keeps the connection
 *   open -- non-fatal: only that registration is refused and the worker keeps
 *   serving its other functions.
 */
export type RegistrationRejectedMessage = {
  message_type: MessageType.RegistrationRejected
  /** Machine-readable rejection code (`WORKER_NAMESPACE_CONFLICT` | `FUNCTION_NAMESPACE_CONFLICT`). */
  code: string
  /** Namespace in which the collision occurred. */
  namespace: string
  /** Contested worker name. Present only for `WORKER_NAMESPACE_CONFLICT`. */
  worker_name?: string
  /** Contested function id. Present only for `FUNCTION_NAMESPACE_CONFLICT`. */
  function_id?: string
  /** ID of the live worker that already owns the contested identity. */
  owner_worker_id: string
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
  | WorkerRegisteredMessage
  | RegistrationRejectedMessage
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
