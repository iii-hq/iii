/**
 * Typed error surfaced when an invocation dispatched over the SDK fails, RBAC
 * rejection (FORBIDDEN), handler-level failure, or a timeout waiting for the
 * engine to respond. Wraps the wire `ErrorBody` shape plus the `function_id`
 * that was targeted, so callers get a single error type across all failure
 * modes and can disambiguate via `err.code`.
 *
 * Before this existed, rejection values were plain `ErrorBody`-shaped objects,
 * which printed as `[object Object]` when stringified, leaving developers to
 * grep through SDK source to figure out what tripped. The class name, `code`
 * prefix in the message, and `function_id` field together make a rejection
 * self-describing.
 */
export type InvocationErrorInit = {
  code: string
  message: string
  function_id?: string
  stacktrace?: string
}

export class InvocationError extends Error {
  public readonly code: string
  public readonly function_id?: string
  public readonly stacktrace?: string

  constructor(init: InvocationErrorInit) {
    super(`${init.code}: ${init.message}`)
    this.name = 'InvocationError'
    this.code = init.code
    this.function_id = init.function_id
    this.stacktrace = init.stacktrace
  }
}

/**
 * A stream failed instead of ending: its writer aborted it (a source that
 * threw, a handler that threw while writing, `out.abort()`), or its socket was
 * lost. Thrown by the reads of an {@link InputStream} after the data sent
 * before the abort, and the error a call rejects with when a sink it passed
 * was aborted (`code` is `STREAM_ABORTED` in both cases).
 *
 * `reason` is what the reader was told. Across the engine that is the
 * engine's text (`channel writer aborted: closed with code 1011`), not the
 * writer's own reason: the engine does not relay it.
 */
export class StreamAbortedError extends InvocationError {
  /** Why the stream was aborted, as the reader learned it. */
  public readonly reason: string

  constructor(reason: string, options?: { function_id?: string; cause?: unknown }) {
    super({ code: 'STREAM_ABORTED', message: `stream aborted: ${reason}`, function_id: options?.function_id })
    this.name = 'StreamAbortedError'
    this.reason = reason
    if (options?.cause !== undefined) (this as { cause?: unknown }).cause = options.cause
  }
}

/** Fields used to construct a {@link RegistrationRejectedError}. */
export type RegistrationRejectedInit = {
  code: string
  namespace: string
  worker_name?: string
  function_id?: string
  owner_worker_id: string
}

/**
 * Fatal error surfaced when the engine rejects this worker's identity because
 * another live worker owns the same `(namespace, worker_name)`, or when an
 * unknown rejection code is received. The SDK stops and does not reconnect.
 * A `FUNCTION_NAMESPACE_CONFLICT` is non-fatal and is logged instead of being
 * surfaced through this class. The wire fields identify the contested
 * registration and its current owner.
 */
export class RegistrationRejectedError extends Error {
  public readonly code: string
  public readonly namespace: string
  public readonly worker_name?: string
  public readonly function_id?: string
  public readonly owner_worker_id: string

  constructor(init: RegistrationRejectedInit) {
    super(
      `${init.code}: registration rejected for ${init.function_id !== undefined ? `function "${init.function_id}"` : `worker "${init.worker_name ?? '<unknown>'}"`} in namespace "${init.namespace}" (already owned by worker ${init.owner_worker_id})`,
    )
    this.name = 'RegistrationRejectedError'
    this.code = init.code
    this.namespace = init.namespace
    this.worker_name = init.worker_name
    this.function_id = init.function_id
    this.owner_worker_id = init.owner_worker_id
  }
}

/**
 * True when `value` looks like the wire `ErrorBody` the engine sends in
 * `InvocationResult.error`: `{ code: string, message: string, stacktrace?: string }`.
 * Used to distinguish an engine rejection (which we wrap in
 * {@link InvocationError}) from a JS `Error` thrown elsewhere.
 */
export function isErrorBody(value: unknown): value is {
  code: string
  message: string
  stacktrace?: string
} {
  if (typeof value !== 'object' || value === null) return false
  const v = value as { code?: unknown; message?: unknown; stacktrace?: unknown }
  return (
    typeof v.code === 'string' &&
    typeof v.message === 'string' &&
    (v.stacktrace === undefined || typeof v.stacktrace === 'string')
  )
}

/**
 * Thrown by an `onObserverAdded` hook to reject an observer with a code of
 * its own (`HTTP_ROUTE_CONFLICT`, `INVALID_CRON_EXPRESSION`, ...); any other
 * error rejects it with `OBSERVE_REJECTED`. Also the error `ObserverHandle.ready()`
 * rejects with when the observer turns `error` or `disposed`.
 *
 * @example
 * ```typescript
 * onObserverAdded(obs) {
 *   if (!isValidCron(obs.args.expression)) {
 *     throw new ObserveRejection('INVALID_CRON_EXPRESSION', 'invalid day-of-week field')
 *   }
 * }
 * ```
 */
export class ObserveRejection extends Error {
  public readonly code: string

  constructor(code: string, message: string) {
    super(message)
    this.name = 'ObserveRejection'
    this.code = code
  }
}
