import { Calendar, ChevronDown, Clock, X } from 'lucide-react'
import { useCallback, useMemo, useState } from 'react'
import { Button, cn, Input } from '@/components/ui/card'
import {
  getTimeRangeFromPreset,
  TIME_RANGE_PRESETS,
  type TimeRange,
  type TimeRangePreset,
} from '@/lib/timeRangeUtils'

// Preset labels for display
const PRESET_LABELS: Record<TimeRangePreset, string> = {
  LAST_15_MINUTES: 'Last 15 minutes',
  LAST_30_MINUTES: 'Last 30 minutes',
  LAST_1_HOUR: 'Last 1 hour',
  LAST_3_HOURS: 'Last 3 hours',
  LAST_6_HOURS: 'Last 6 hours',
  LAST_12_HOURS: 'Last 12 hours',
  LAST_24_HOURS: 'Last 24 hours',
  LAST_7_DAYS: 'Last 7 days',
}

// Short preset labels for compact display
const PRESET_SHORT_LABELS: Record<TimeRangePreset, string> = {
  LAST_15_MINUTES: '15m',
  LAST_30_MINUTES: '30m',
  LAST_1_HOUR: '1h',
  LAST_3_HOURS: '3h',
  LAST_6_HOURS: '6h',
  LAST_12_HOURS: '12h',
  LAST_24_HOURS: '24h',
  LAST_7_DAYS: '7d',
}

// Props for TimeRangeFilter component
export interface TimeRangeFilterProps {
  value?: TimeRange
  onChange?: (range: TimeRange | undefined) => void
  onClear?: () => void
  className?: string
  showCustomOption?: boolean
  compactMode?: boolean
}

// Format timestamp for datetime-local input
function formatDateTimeLocal(timestamp: number): string {
  const date = new Date(timestamp)
  // Format as YYYY-MM-DDTHH:mm for datetime-local input
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  return `${year}-${month}-${day}T${hours}:${minutes}`
}

// Parse datetime-local input to timestamp
function parseDateTimeLocal(value: string): number {
  return new Date(value).getTime()
}

// Format a time range for display
function formatTimeRangeDisplay(range: TimeRange): string {
  if (range.preset && range.preset !== 'custom') {
    return PRESET_LABELS[range.preset]
  }

  const startDate = new Date(range.startTime)
  const endDate = new Date(range.endTime)

  const formatOptions: Intl.DateTimeFormatOptions = {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }

  return `${startDate.toLocaleDateString('en-US', formatOptions)} - ${endDate.toLocaleDateString('en-US', formatOptions)}`
}

/**
 * TimeRangeFilter component for filtering by time range.
 *
 * Supports preset time ranges (last 15m, 1h, 24h, etc.) and custom date/time ranges.
 *
 * @example
 * ```tsx
 * // With preset
 * const [timeRange, setTimeRange] = useState<TimeRange>();
 *
 * <TimeRangeFilter
 *   value={timeRange}
 *   onChange={setTimeRange}
 * />
 *
 * // Compact mode for filter bars
 * <TimeRangeFilter
 *   value={timeRange}
 *   onChange={setTimeRange}
 *   compactMode
 * />
 * ```
 */
export function TimeRangeFilter({
  value,
  onChange,
  onClear,
  className = '',
  showCustomOption = true,
  compactMode = false,
}: TimeRangeFilterProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [isCustomMode, setIsCustomMode] = useState(value?.preset === 'custom')
  const [customStart, setCustomStart] = useState<string>(() => {
    if (value?.preset === 'custom') {
      return formatDateTimeLocal(value.startTime)
    }
    // Default to 1 hour ago
    return formatDateTimeLocal(Date.now() - TIME_RANGE_PRESETS.LAST_1_HOUR)
  })
  const [customEnd, setCustomEnd] = useState<string>(() => {
    if (value?.preset === 'custom') {
      return formatDateTimeLocal(value.endTime)
    }
    return formatDateTimeLocal(Date.now())
  })

  // Handle preset selection
  const handlePresetSelect = useCallback(
    (preset: TimeRangePreset) => {
      const { startTime, endTime } = getTimeRangeFromPreset(preset)
      onChange?.({
        startTime,
        endTime,
        preset,
      })
      setIsCustomMode(false)
      setIsOpen(false)
    },
    [onChange],
  )

  // Handle custom range application
  const handleApplyCustom = useCallback(() => {
    // Validate both inputs are present
    if (!customStart || !customEnd) {
      return
    }

    const startTime = parseDateTimeLocal(customStart)
    const endTime = parseDateTimeLocal(customEnd)

    // Validate parsed values are valid numbers
    if (Number.isNaN(startTime) || Number.isNaN(endTime)) {
      return
    }

    // Validate start is before end
    if (startTime >= endTime) {
      return
    }

    onChange?.({
      startTime,
      endTime,
      preset: 'custom',
    })
    setIsOpen(false)
  }, [customStart, customEnd, onChange])

  // Handle clear
  const handleClear = useCallback(() => {
    onChange?.(undefined)
    onClear?.()
    setIsCustomMode(false)
    setIsOpen(false)
  }, [onChange, onClear])

  // Enter custom mode
  const handleEnterCustomMode = useCallback(() => {
    setIsCustomMode(true)
    // Update custom inputs with current time range
    if (value) {
      setCustomStart(formatDateTimeLocal(value.startTime))
      setCustomEnd(formatDateTimeLocal(value.endTime))
    } else {
      setCustomStart(formatDateTimeLocal(Date.now() - TIME_RANGE_PRESETS.LAST_1_HOUR))
      setCustomEnd(formatDateTimeLocal(Date.now()))
    }
  }, [value])

  // Get display text
  const displayText = useMemo(() => {
    if (!value) {
      return compactMode ? 'Time' : 'Select time range'
    }
    if (compactMode && value.preset && value.preset !== 'custom') {
      return PRESET_SHORT_LABELS[value.preset]
    }
    return formatTimeRangeDisplay(value)
  }, [value, compactMode])

  // Preset options ordered by duration
  const presetOptions: TimeRangePreset[] = [
    'LAST_15_MINUTES',
    'LAST_30_MINUTES',
    'LAST_1_HOUR',
    'LAST_3_HOURS',
    'LAST_6_HOURS',
    'LAST_12_HOURS',
    'LAST_24_HOURS',
    'LAST_7_DAYS',
  ]

  return (
    <div className={cn('relative', className)}>
      {/* Trigger button */}
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'flex items-center gap-2 px-3 py-1.5 rounded text-xs transition-colors',
          'bg-[#1D1D1D] border border-[#1D1D1D] hover:border-[#5B5B5B]',
          value ? 'text-[#F4F4F4]' : 'text-[#5B5B5B]',
          isOpen && 'border-[#5B5B5B]',
        )}
      >
        <Clock className="w-3 h-3" />
        <span className="truncate max-w-[200px]">{displayText}</span>
        <ChevronDown className={cn('w-3 h-3 transition-transform', isOpen && 'rotate-180')} />
      </button>
      {value && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            handleClear()
          }}
          className="absolute right-7 top-1/2 -translate-y-1/2 p-0.5 hover:bg-[#5B5B5B]/30 rounded"
          title="Clear time filter"
        >
          <X className="w-3 h-3" />
        </button>
      )}

      {/* Dropdown */}
      {isOpen && (
        <div
          className={cn(
            'absolute z-50 mt-1 w-64 rounded border border-[#1D1D1D] bg-[#0A0A0A] shadow-lg',
            'overflow-hidden',
          )}
        >
          {/* Presets section */}
          {!isCustomMode && (
            <div className="p-2">
              <div className="text-[10px] font-medium text-[#5B5B5B] uppercase tracking-wider mb-2 px-2">
                Quick Select
              </div>
              <div className="space-y-0.5">
                {presetOptions.map((preset) => (
                  <button
                    key={preset}
                    type="button"
                    onClick={() => handlePresetSelect(preset)}
                    className={cn(
                      'w-full text-left px-2 py-1.5 rounded text-xs transition-colors',
                      value?.preset === preset
                        ? 'bg-[#F3F724]/20 text-[#F3F724]'
                        : 'text-[#F4F4F4] hover:bg-[#1D1D1D]',
                    )}
                  >
                    {PRESET_LABELS[preset]}
                  </button>
                ))}
              </div>

              {/* Custom option */}
              {showCustomOption && (
                <>
                  <div className="border-t border-[#1D1D1D] my-2" />
                  <button
                    type="button"
                    onClick={handleEnterCustomMode}
                    className={cn(
                      'w-full flex items-center gap-2 px-2 py-1.5 rounded text-xs transition-colors',
                      value?.preset === 'custom'
                        ? 'bg-[#F3F724]/20 text-[#F3F724]'
                        : 'text-[#F4F4F4] hover:bg-[#1D1D1D]',
                    )}
                  >
                    <Calendar className="w-3 h-3" />
                    Custom Range
                  </button>
                </>
              )}
            </div>
          )}

          {/* Custom range inputs */}
          {isCustomMode && (
            <div className="p-3 space-y-3">
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setIsCustomMode(false)}
                  className="text-[10px] text-[#5B5B5B] hover:text-[#F4F4F4] transition-colors"
                >
                  &larr; Back
                </button>
                <span className="text-[10px] font-medium text-[#5B5B5B] uppercase tracking-wider">
                  Custom Range
                </span>
              </div>

              <div className="space-y-2">
                <div>
                  <label
                    htmlFor="time-range-start"
                    className="text-[10px] text-[#5B5B5B] uppercase tracking-wider mb-1 block"
                  >
                    Start
                  </label>
                  <Input
                    id="time-range-start"
                    type="datetime-local"
                    value={customStart}
                    onChange={(e) => setCustomStart(e.target.value)}
                    className="w-full text-[11px]"
                  />
                </div>

                <div>
                  <label
                    htmlFor="time-range-end"
                    className="text-[10px] text-[#5B5B5B] uppercase tracking-wider mb-1 block"
                  >
                    End
                  </label>
                  <Input
                    id="time-range-end"
                    type="datetime-local"
                    value={customEnd}
                    onChange={(e) => setCustomEnd(e.target.value)}
                    className="w-full text-[11px]"
                  />
                </div>
              </div>

              <div className="flex items-center gap-2 pt-1">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setIsCustomMode(false)}
                  className="flex-1"
                >
                  Cancel
                </Button>
                <Button
                  type="button"
                  variant="accent"
                  size="sm"
                  onClick={handleApplyCustom}
                  className="flex-1"
                >
                  Apply
                </Button>
              </div>
            </div>
          )}

          {/* Clear option */}
          {value && !isCustomMode && (
            <>
              <div className="border-t border-[#1D1D1D]" />
              <button
                type="button"
                onClick={handleClear}
                className="w-full px-4 py-2 text-xs text-[#5B5B5B] hover:text-[#F4F4F4] hover:bg-[#1D1D1D] transition-colors text-left"
              >
                Clear filter
              </button>
            </>
          )}
        </div>
      )}

      {/* Click-away handler */}
      {isOpen && (
        // biome-ignore lint/a11y/noStaticElementInteractions: click-away overlay with keyboard support
        <div
          role="presentation"
          className="fixed inset-0 z-40"
          onClick={() => setIsOpen(false)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') setIsOpen(false)
          }}
        />
      )}
    </div>
  )
}

// Export types for external use
export type { TimeRange }
