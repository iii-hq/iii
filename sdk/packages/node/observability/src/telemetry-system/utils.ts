/**
 * Parse a numeric environment variable with optional minimum bound.
 *
 * An empty or whitespace-only string is treated as unset: `Number('')` is 0,
 * so without this check a variable set-but-blank (common in .env files) would
 * silently resolve to 0 instead of falling through to the default.
 */
export function parseNumberEnv(value: string | undefined, minimum: number = 0): number | undefined {
  if (value === undefined || value.trim() === '') return undefined
  const parsed = Number(value)
  if (!Number.isFinite(parsed) || parsed < minimum) return undefined
  return parsed
}

/**
 * Parse an integer environment variable with optional minimum bound.
 */
export function parseIntegerEnv(
  value: string | undefined,
  minimum: number = 0,
): number | undefined {
  const parsed = parseNumberEnv(value, minimum)
  if (parsed === undefined || !Number.isInteger(parsed)) return undefined
  return parsed
}

/**
 * Resolve a batch-processor flush delay: explicit config wins, then the
 * III-specific env var, then the III default.
 *
 * III SDKs deliberately expose a single OTEL_*_FLUSH_INTERVAL_MS knob and do
 * NOT honor the standard OTel OTEL_BSP_SCHEDULE_DELAY / OTEL_BLRP_SCHEDULE_DELAY
 * vars, for cross-SDK consistency (the Node, Python, and Rust SDKs all resolve
 * the same way). Passing an explicit `scheduledDelayMillis` to the processor
 * already makes the OTel SDK ignore those standard vars regardless.
 */
export function resolveFlushIntervalMs(
  configValue: number | undefined,
  envValue: string | undefined,
  defaultValue: number,
): number {
  return configValue ?? parseNumberEnv(envValue, 0) ?? defaultValue
}
