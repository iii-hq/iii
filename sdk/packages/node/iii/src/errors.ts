/**
 * Typed error surfaced when an invocation dispatched over the SDK fails — RBAC
 * rejection (FORBIDDEN), handler-level failure, or a timeout waiting for the
 * engine to respond. Wraps the wire `ErrorBody` shape plus the `function_id`
 * that was targeted, so callers get a single error type across all failure
 * modes and can disambiguate via `err.code`.
 *
 * Before this existed, rejection values were plain `ErrorBody`-shaped objects,
 * which printed as `[object Object]` when stringified — leaving developers to
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
 * @deprecated Renamed to {@link InvocationError}; import from `iii-sdk/errors`.
 */
export const IIIInvocationError = InvocationError
/**
 * @deprecated Renamed to {@link InvocationError}.
 */
export type IIIInvocationError = InvocationError
/**
 * @deprecated Renamed to {@link InvocationErrorInit}.
 */
export type IIIInvocationErrorInit = InvocationErrorInit
