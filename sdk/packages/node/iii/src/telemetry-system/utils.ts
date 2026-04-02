/**
 * Parse a numeric environment variable with optional minimum bound.
 */
export function parseNumberEnv(value: string | undefined, minimum: number = 0): number | undefined {
  if (value === undefined) return undefined
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
