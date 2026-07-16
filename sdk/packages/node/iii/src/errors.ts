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
 * Fatal error surfaced when the engine rejects this worker's registration with
 * a `registrationrejected` message: another live worker already owns this
 * `worker_name` (or an exported function id) in the target `namespace`. The
 * engine closes the connection on rejection, so this is terminal -- the SDK
 * stops the worker and does not reconnect. Carries the wire fields
 * (`code`, `namespace`, `worker_name`, `owner_worker_id`) so callers can tell
 * exactly which identity collided and with whom.
 */
export type RegistrationRejectedInit = {
  code: string
  namespace: string
  worker_name: string
  owner_worker_id: string
}

export class RegistrationRejectedError extends Error {
  public readonly code: string
  public readonly namespace: string
  public readonly worker_name: string
  public readonly owner_worker_id: string

  constructor(init: RegistrationRejectedInit) {
    super(
      `${init.code}: registration rejected for worker "${init.worker_name}" in namespace "${init.namespace}" (already owned by worker ${init.owner_worker_id})`,
    )
    this.name = 'RegistrationRejectedError'
    this.code = init.code
    this.namespace = init.namespace
    this.worker_name = init.worker_name
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
