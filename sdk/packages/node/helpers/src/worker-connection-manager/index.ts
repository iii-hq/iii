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
   * `engine::workers::register`. Omit this map to add no namespace-scoped
   * grants: the session may declare any namespace, and only
   * `allowed_functions` and `expose_functions` apply.
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
