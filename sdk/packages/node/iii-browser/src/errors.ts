export type RegistrationRejectedInit = {
  code: string
  namespace: string
  worker_name?: string
  function_id?: string
  owner_worker_id: string
}

/**
 * A terminal registration rejection from the engine. Thrown into every pending
 * invocation and exposed via `getFatalError()` when a worker-name collision
 * (or other non-retryable rejection) closes the connection for good.
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

/** Fields used to construct an {@link InvocationError}. */
export type InvocationErrorInit = {
  code: string
  message: string
  function_id?: string
  stacktrace?: string
}

/**
 * An error the SDK raises itself for an invocation, with a machine-readable
 * `code`: observe delivery errors (`NOT_OWNER_DELIVERED`,
 * `OBSERVER_NOT_ACTIVE`, `FILTERS_NOT_MATCHED`) and an invalid observe
 * descriptor (`INVALID_OBSERVE_DESCRIPTOR`). Mirrors the Node SDK.
 *
 * A call the engine answers with an error still rejects with the engine's
 * error body (`{ code, message, stacktrace? }`), as in earlier releases, and
 * so does a locally dispatched call whose handler throws: both paths reject
 * with the same value.
 */
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
