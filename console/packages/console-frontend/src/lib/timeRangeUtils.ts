/**
 * Time range presets in milliseconds.
 * Standalone utility - no React or hook dependencies.
 */
export const TIME_RANGE_PRESETS = {
  LAST_15_MINUTES: 15 * 60 * 1000,
  LAST_30_MINUTES: 30 * 60 * 1000,
  LAST_1_HOUR: 60 * 60 * 1000,
  LAST_3_HOURS: 3 * 60 * 60 * 1000,
  LAST_6_HOURS: 6 * 60 * 60 * 1000,
  LAST_12_HOURS: 12 * 60 * 60 * 1000,
  LAST_24_HOURS: 24 * 60 * 60 * 1000,
  LAST_7_DAYS: 7 * 24 * 60 * 60 * 1000,
} as const

export type TimeRangePreset = keyof typeof TIME_RANGE_PRESETS

export interface TimeRange {
  startTime: number
  endTime: number
  preset?: TimeRangePreset | 'custom'
}

/**
 * Calculate start and end timestamps from a preset.
 */
export function getTimeRangeFromPreset(preset: TimeRangePreset): {
  startTime: number
  endTime: number
} {
  const endTime = Date.now()
  const startTime = endTime - TIME_RANGE_PRESETS[preset]
  return { startTime, endTime }
}
