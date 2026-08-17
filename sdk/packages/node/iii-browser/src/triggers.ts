/**
 * Configuration passed to a trigger handler when a trigger instance is
 * registered or unregistered.
 *
 * @typeParam TConfig - Type of the trigger-specific configuration.
 */
export type TriggerConfig<TConfig> = {
  /** Trigger instance ID. */
  id: string
  /** Function to invoke when the trigger fires. */
  function_id: string
  /** Trigger-specific configuration. */
  config: TConfig
  /**
   * Namespace the trigger's target `function_id` resolves in. A provider that
   * stores this config and later fires the target must pass this namespace, or
   * it fires in `default`.
   *
   * Omitted, it is filled in with the registering worker's namespace. A trigger
   * names a function, and a worker's functions land in the worker's namespace,
   * so defaulting anywhere else registers a trigger that resolves nothing. Name
   * another namespace, `default` included, to target one.
   */
  namespace?: string
}

/**
 * Handler interface for custom trigger types. Passed to
 * `ISdk.registerTriggerType`.
 *
 * @typeParam TConfig - Type of the trigger-specific configuration.
 *
 * @example
 * ```typescript
 * const handler: TriggerHandler<{ interval: number }> = {
 *   async registerTrigger({ id, function_id, config }) {
 *     // Set up periodic invocation
 *   },
 *   async unregisterTrigger({ id, function_id, config }) {
 *     // Clean up
 *   },
 * }
 * ```
 */
export type TriggerHandler<TConfig> = {
  /** Called when a trigger instance is registered. */
  registerTrigger(config: TriggerConfig<TConfig>): Promise<void>
  /** Called when a trigger instance is unregistered. */
  unregisterTrigger(config: TriggerConfig<TConfig>): Promise<void>
}
