import type { ErrorBody } from './iii-types'

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
 * `code`: `LOCAL_DEPTH_EXCEEDED` (a local call nested too deep while the
 * socket is closed) and `payload_too_large`. Mirrors the Node SDK.
 *
 * A call the engine answers with an error rejects with the engine's error
 * body (`{ code, message, stacktrace? }`), and so does a locally dispatched
 * call whose handler or deliver hook throws: both paths reject with the same
 * value.
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

/** What a failure reports when the thrown value cannot even be read (a getter or `toString` that throws). */
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
 * The error a handler's (or a deliver hook's) failure becomes, the same on the
 * local and the remote path (code `invocation_failed`):
 * - an `Error`, from any realm (an iframe's too): its message and stack;
 * - an engine error body (a failed call's rejection, rethrown): its message and stacktrace;
 * - anything else: `String(value)`.
 * Never throws: when reading the value throws, a fixed message is reported, so
 * the caller always gets its answer.
 */
export function handlerErrorBody(error: unknown): ErrorBody {
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

/**
 * Thrown by an `onObserverAdded` hook to reject an observer with a code of
 * its own (`HTTP_ROUTE_CONFLICT`, `INVALID_CRON_EXPRESSION`, ...); any other
 * error rejects it with `OBSERVE_REJECTED`. Also the error `ObserverHandle.ready()`
 * rejects with when the observer turns `error` or `disposed`.
 *
 * @example
 * ```typescript
 * iii.onObserverAdded('cron::on-tick', (obs) => {
 *   if (!isValidCron(obs.args.expression)) {
 *     throw new ObserveRejection('INVALID_CRON_EXPRESSION', 'invalid day-of-week field')
 *   }
 * })
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
