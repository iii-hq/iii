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
  /** Additional function IDs to allow beyond the `expose_functions` config. Defaults to `[]` if omitted. */
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
 * All fields are optional -- omitted fields keep the original value from the
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
  /** Arbitrary metadata attached to the trigger. */
  metadata?: Record<string, unknown>
  /** Auth context from `AuthResult.context` for this session. */
  context: Record<string, unknown>
}

/**
 * Result returned from the `on_trigger_registration_function_id` hook.
 * All fields are optional -- omitted fields keep the original value from the
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
  /** Auth context from `AuthResult.context` for this session. */
  context: Record<string, unknown>
}

/**
 * Result returned from the `on_function_registration_function_id` hook.
 * All fields are optional -- omitted fields keep the original value from the
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
