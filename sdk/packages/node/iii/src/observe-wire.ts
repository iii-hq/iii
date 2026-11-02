/**
 * Builders and decoders for the observe messages (protocol.md §2,
 * `sdk/fixtures/protocol/observe`). Optional fields are left out, never sent
 * as `null`; decoders accept `null` for them, keep enum values they do not
 * know, and fill the documented defaults.
 */
import type {
  ActivationWire,
  CallAction,
  CallerWire,
  CaptureWire,
  ErrorBody,
  FunctionRegisteredMessage,
  InvocationResultMessage,
  InvokeFunctionMessage,
  JsonValue,
  ObserveMessage,
  ObserveResultMessage,
  ObserverAckMessage,
  ObserverAddedMessage,
  ObserverInfoWire,
  ObserverRemovedMessage,
  ObserverStateChangedMessage,
  RegistrationRejectedMessage,
  WorkerRegisteredMessage,
} from './iii-types'
import type { ObserverOptions, ObserveSource, Target } from './observe-types'

/** CONTRACT D1: what an absent `capture` means. */
export const DEFAULT_CAPTURE: readonly CaptureWire[] = ['input', 'output', 'caller']

type Raw = Record<string, unknown>

function isRecord(v: unknown): v is Raw {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

function str(v: unknown): string | undefined {
  return typeof v === 'string' ? v : undefined
}

function num(v: unknown): number | undefined {
  return typeof v === 'number' ? v : undefined
}

function errorBody(v: unknown): ErrorBody | undefined {
  if (!isRecord(v)) return undefined
  const body: ErrorBody = { code: String(v.code ?? ''), message: String(v.message ?? '') }
  if (typeof v.stacktrace === 'string') body.stacktrace = v.stacktrace
  return body
}

/** Copies the wire caller, dropping `null`s and a `parent` inside `parent`. */
export function decodeCaller(v: unknown, depth = 0): CallerWire | undefined {
  if (!isRecord(v)) return undefined
  const caller: CallerWire = {}
  for (const key of ['worker_id', 'worker_name', 'function_id', 'invocation_id'] as const) {
    const value = str(v[key])
    if (value !== undefined) caller[key] = value
  }
  if (depth === 0) {
    const parent = decodeCaller(v.parent, 1)
    if (parent) caller.parent = parent
  }
  return caller
}

function decodeActivation(v: unknown): ActivationWire | undefined {
  if (!isRecord(v) || typeof v.observer_id !== 'string') return undefined
  const activation: ActivationWire = { observer_id: v.observer_id }
  if (typeof v.activation_id === 'string') activation.activation_id = v.activation_id
  return activation
}

/** An observer as its host sees it (`observeradded.observer`, `functionregistered.observers[]`). */
export function decodeObserverInfo(v: unknown): ObserverInfoWire {
  const raw = isRecord(v) ? v : {}
  const info: ObserverInfoWire = {
    id: String(raw.id ?? ''),
    args: isRecord(raw.args) ? raw.args : {},
    // Absent is `{}`; anything else is compiled as sent, so a malformed one is
    // acked INVALID_FILTER rather than read as match-all.
    filters: (raw.filters === undefined ? {} : raw.filters) as Record<string, unknown>,
    target: String(raw.target ?? ''),
    capture: Array.isArray(raw.capture)
      ? raw.capture.filter((c): c is string => typeof c === 'string')
      : [...DEFAULT_CAPTURE],
    on: str(raw.on) ?? 'success',
  }
  return info
}

export function decodeFunctionRegistered(v: Raw): Omit<FunctionRegisteredMessage, 'message_type'> {
  return {
    id: String(v.id ?? ''),
    namespace: String(v.namespace ?? ''),
    revision: num(v.revision) ?? 0,
    local_dispatch: v.local_dispatch === true,
    observers: Array.isArray(v.observers) ? v.observers.map(decodeObserverInfo) : [],
  }
}

export function decodeObserverAdded(v: Raw): Omit<ObserverAddedMessage, 'message_type'> {
  return {
    function_id: String(v.function_id ?? ''),
    namespace: String(v.namespace ?? ''),
    revision: num(v.revision) ?? 0,
    observer: decodeObserverInfo(v.observer),
  }
}

export function decodeObserverRemoved(v: Raw): Omit<ObserverRemovedMessage, 'message_type'> {
  return {
    function_id: String(v.function_id ?? ''),
    namespace: String(v.namespace ?? ''),
    revision: num(v.revision) ?? 0,
    id: String(v.id ?? ''),
  }
}

export function decodeObserveResult(v: Raw): Omit<ObserveResultMessage, 'message_type'> {
  const msg: Omit<ObserveResultMessage, 'message_type'> = { id: String(v.id ?? ''), state: String(v.state ?? '') }
  const error = errorBody(v.error)
  if (error) msg.error = error
  if (Array.isArray(v.warnings)) msg.warnings = v.warnings.filter((w): w is string => typeof w === 'string')
  return msg
}

export function decodeObserverStateChanged(v: Raw): Omit<ObserverStateChangedMessage, 'message_type'> {
  const msg: Omit<ObserverStateChangedMessage, 'message_type'> = {
    id: String(v.id ?? ''),
    state: String(v.state ?? ''),
    fires: num(v.fires) ?? 0,
  }
  const reason = errorBody(v.reason)
  if (reason) msg.reason = reason
  return msg
}

export function decodeWorkerRegistered(v: Raw): Omit<WorkerRegisteredMessage, 'message_type'> {
  const msg: Omit<WorkerRegisteredMessage, 'message_type'> = { worker_id: String(v.worker_id ?? '') }
  if (typeof v.reattach_token === 'string') msg.reattach_token = v.reattach_token
  if (Array.isArray(v.features)) msg.features = v.features.filter((f): f is string => typeof f === 'string')
  return msg
}

/** The observe fields of an inbound `invokefunction` (`caller`, `observe_depth`, `activation`). */
export function decodeInvokeFunction(v: Raw): Omit<InvokeFunctionMessage, 'message_type'> {
  const msg: Omit<InvokeFunctionMessage, 'message_type'> = {
    function_id: String(v.function_id ?? ''),
    data: v.data,
  }
  if (typeof v.invocation_id === 'string') msg.invocation_id = v.invocation_id
  if (typeof v.traceparent === 'string') msg.traceparent = v.traceparent
  if (typeof v.baggage === 'string') msg.baggage = v.baggage
  if (isRecord(v.action)) msg.action = v.action as CallAction
  if (v.metadata !== undefined && v.metadata !== null) msg.metadata = v.metadata as JsonValue
  if (typeof v.namespace === 'string') msg.namespace = v.namespace
  const caller = decodeCaller(v.caller)
  if (caller) msg.caller = caller
  const depth = num(v.observe_depth)
  if (depth !== undefined && depth > 0) msg.observe_depth = depth
  const activation = decodeActivation(v.activation)
  if (activation) msg.activation = activation
  return msg
}

/**
 * An `invocationresult`. `result: null` is a value; an absent one is a
 * function that returned nothing. `matched` and `delivered` are read as the
 * engine reads them (decision 55): `matched: null` is absent, an array is its
 * string ids (`[]` included), and any other value is `[]`; `delivered` is
 * kept only when it is `true`.
 */
export function decodeInvocationResult(v: Raw): Omit<InvocationResultMessage, 'message_type'> {
  const msg: Omit<InvocationResultMessage, 'message_type'> = {
    invocation_id: String(v.invocation_id ?? ''),
    function_id: String(v.function_id ?? ''),
  }
  if (v.result !== undefined) msg.result = v.result
  const error = errorBody(v.error)
  if (error) msg.error = error
  if (typeof v.traceparent === 'string') msg.traceparent = v.traceparent
  if (typeof v.baggage === 'string') msg.baggage = v.baggage
  if (v.matched !== undefined && v.matched !== null) {
    msg.matched = Array.isArray(v.matched) ? v.matched.filter((id): id is string => typeof id === 'string') : []
  }
  if (v.delivered === true) msg.delivered = true
  return msg
}

export function decodeRegistrationRejected(v: Raw): Omit<RegistrationRejectedMessage, 'message_type'> {
  const msg: Omit<RegistrationRejectedMessage, 'message_type'> = {
    code: String(v.code ?? ''),
    namespace: String(v.namespace ?? ''),
    owner_worker_id: String(v.owner_worker_id ?? ''),
  }
  if (typeof v.worker_name === 'string') msg.worker_name = v.worker_name
  if (typeof v.function_id === 'string') msg.function_id = v.function_id
  return msg
}

/** Drops `parent` (one level only). */
export function stripCaller(caller: CallerWire | undefined): Omit<CallerWire, 'parent'> | undefined {
  if (!caller) return undefined
  const { parent: _, ...rest } = caller
  return rest
}

/** JSON's view of a message: `undefined` fields left out, no caller-owned objects retained. */
function snapshot<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

/**
 * `observe { id, source, target, ... }` from the three arguments of
 * `iii.observe`. Only what the caller set is sent; the engine applies the
 * defaults. `resolveNamespace` gives the namespace of an unqualified id (this
 * worker's, `default` for `engine::*`).
 */
export function buildObserveMessage(
  id: string,
  source: ObserveSource,
  target: Target,
  options: ObserverOptions | undefined,
  resolveNamespace: (explicit: string | undefined, functionId: string, field: string) => string | undefined,
): Omit<ObserveMessage, 'message_type'> {
  const targetRef = typeof target === 'string' ? { function_id: target } : target
  const sourceNamespace = resolveNamespace(source.namespace, source.function_id, 'ObserveSource.namespace')
  const targetNamespace = resolveNamespace(targetRef.namespace, targetRef.function_id, 'Target.namespace')
  return snapshot({
    id,
    source: {
      function_id: source.function_id,
      namespace: sourceNamespace,
      args: source.args,
      filters: source.filters,
      capture: source.capture,
      on: source.on,
      deliver: source.deliver,
    },
    target: { function_id: targetRef.function_id, namespace: targetNamespace },
    metadata: options?.metadata,
    max_fires: options?.max_fires,
    expires_in_ms: options?.expires_in_ms,
    max_in_flight: options?.max_in_flight,
  })
}

export function buildObserverAck(ack: {
  function_id: string
  namespace?: string
  id: string
  revision: number
  error?: { code: string; message: string }
}): Omit<ObserverAckMessage, 'message_type'> {
  const msg: Omit<ObserverAckMessage, 'message_type'> = {
    function_id: ack.function_id,
    id: ack.id,
    revision: ack.revision,
  }
  if (ack.namespace !== undefined) msg.namespace = ack.namespace
  if (ack.error) msg.error = { code: ack.error.code, message: ack.error.message }
  return msg
}

/**
 * An outbound `invokefunction`. `observe_depth` is left out when 0, and every
 * optional field when unset.
 */
export function buildInvokeFunction(init: {
  invocation_id?: string
  function_id: string
  data: unknown
  traceparent?: string
  baggage?: string
  action?: CallAction
  metadata?: unknown
  namespace?: string
  caller?: CallerWire
  observe_depth?: number
  activation?: ActivationWire
}): Record<string, unknown> {
  const msg: Record<string, unknown> = {}
  if (init.invocation_id !== undefined) msg.invocation_id = init.invocation_id
  msg.function_id = init.function_id
  msg.data = init.data
  if (init.traceparent !== undefined) msg.traceparent = init.traceparent
  if (init.baggage !== undefined) msg.baggage = init.baggage
  if (init.action !== undefined) msg.action = init.action
  if (init.metadata !== undefined) msg.metadata = init.metadata
  if (init.namespace !== undefined) msg.namespace = init.namespace
  if (init.caller !== undefined) msg.caller = init.caller
  if (init.observe_depth !== undefined && init.observe_depth > 0) msg.observe_depth = init.observe_depth
  if (init.activation !== undefined) msg.activation = init.activation
  return msg
}

/**
 * An outbound `invocationresult`: `result` is left out when the function
 * returned nothing (`undefined`). A routed call's answer says how the call
 * is delivered (decision 55): `delivered: true` (its deliver hook delivered
 * it), or `matched` (the observers its match hook matched, `[]` when none),
 * or neither; `delivered` outweighs `matched`, and only one is sent.
 */
export function buildInvocationResult(init: {
  invocation_id: string
  function_id: string
  result?: unknown
  error?: ErrorBody
  traceparent?: string
  baggage?: string
  matched?: readonly string[]
  delivered?: boolean
}): Record<string, unknown> {
  const msg: Record<string, unknown> = { invocation_id: init.invocation_id, function_id: init.function_id }
  if (init.result !== undefined) msg.result = init.result
  if (init.error !== undefined) msg.error = init.error
  if (init.traceparent !== undefined) msg.traceparent = init.traceparent
  if (init.baggage !== undefined) msg.baggage = init.baggage
  if (init.delivered === true) msg.delivered = true
  else if (init.matched !== undefined) msg.matched = [...init.matched]
  return msg
}

/** The caller an outbound call asserts from inside a handler: that handler, and its own caller one level up. */
export function callerFor(ctx: { function_id: string; invocation_id: string; caller?: CallerWire }): CallerWire {
  const caller: CallerWire = { function_id: ctx.function_id, invocation_id: ctx.invocation_id }
  const parent = stripCaller(ctx.caller)
  if (parent && Object.keys(parent).length > 0) caller.parent = parent
  return caller
}
