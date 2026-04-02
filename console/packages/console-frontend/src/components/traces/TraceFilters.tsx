'use client'

import {
  AlertTriangle,
  ArrowUpDown,
  CircleDot,
  Clock,
  Hash,
  Search,
  Server,
  Tags,
  Timer,
  Workflow,
  X,
  XCircle,
} from 'lucide-react'
import { useEffect, useReducer, useRef, useState } from 'react'
import type { TraceFilterState } from '@/hooks/useTraceFilters'
import { AttributesFilter } from './AttributesFilter'

interface TraceFiltersProps {
  filters: TraceFilterState
  onFilterChange: (key: keyof TraceFilterState, value: unknown) => void
  onClear: () => void
  validationWarnings?: {
    durationSwapped?: boolean
    timeRangeSwapped?: boolean
  }
  onClearWarnings?: () => void
  isLoading?: boolean
  searchQuery?: string
  onSearchChange?: (value: string) => void
  stats?: {
    totalTraces: number
    errorCount: number
    avgDuration: number
  }
}

const timeRangePresets = [
  { label: 'Last 15m', value: 15 * 60 * 1000 },
  { label: 'Last 1h', value: 60 * 60 * 1000 },
  { label: 'Last 6h', value: 6 * 60 * 60 * 1000 },
  { label: 'Last 24h', value: 24 * 60 * 60 * 1000 },
  { label: 'Last 7d', value: 7 * 24 * 60 * 60 * 1000 },
]

const sortByOptions = [
  { label: 'Start Time', value: 'start_time' },
  { label: 'Duration', value: 'duration' },
  { label: 'Service Name', value: 'service_name' },
]

const statusOptions = [
  { label: 'All', value: null },
  { label: 'OK', value: 'ok' },
  { label: 'Error', value: 'error' },
  { label: 'Unset', value: 'unset' },
]

type PopoverType = 'status' | 'timeRange' | 'duration' | 'service' | 'operation' | 'sort' | null

// --- Temp Inputs Reducer ---
interface TempInputsState {
  tempServiceName: string
  tempOperationName: string
  tempMinDuration: string
  tempMaxDuration: string
}

type TempInputsAction =
  | { type: 'SET_SERVICE_NAME'; payload: string }
  | { type: 'SET_OPERATION_NAME'; payload: string }
  | { type: 'SET_MIN_DURATION'; payload: string }
  | { type: 'SET_MAX_DURATION'; payload: string }
  | { type: 'CLEAR_DURATION' }
  | { type: 'CLEAR_SERVICE_NAME' }
  | { type: 'CLEAR_OPERATION_NAME' }
  | {
      type: 'SYNC_FROM_FILTERS'
      payload: {
        serviceName?: string
        operationName?: string
        minDurationMs?: number | null
        maxDurationMs?: number | null
      }
    }

function tempInputsReducer(state: TempInputsState, action: TempInputsAction): TempInputsState {
  switch (action.type) {
    case 'SET_SERVICE_NAME':
      return { ...state, tempServiceName: action.payload }
    case 'SET_OPERATION_NAME':
      return { ...state, tempOperationName: action.payload }
    case 'SET_MIN_DURATION':
      return { ...state, tempMinDuration: action.payload }
    case 'SET_MAX_DURATION':
      return { ...state, tempMaxDuration: action.payload }
    case 'CLEAR_DURATION':
      return { ...state, tempMinDuration: '', tempMaxDuration: '' }
    case 'CLEAR_SERVICE_NAME':
      return { ...state, tempServiceName: '' }
    case 'CLEAR_OPERATION_NAME':
      return { ...state, tempOperationName: '' }
    case 'SYNC_FROM_FILTERS':
      return {
        tempServiceName: action.payload.serviceName || '',
        tempOperationName: action.payload.operationName || '',
        tempMinDuration: action.payload.minDurationMs?.toString() || '',
        tempMaxDuration: action.payload.maxDurationMs?.toString() || '',
      }
    default:
      return state
  }
}

function useClickOutside(ref: React.RefObject<HTMLElement | null>, onClickOutside: () => void) {
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        onClickOutside()
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
    }
  }, [ref, onClickOutside])
}

function formatDuration(ms: number | undefined): string {
  if (!ms) return '-'
  if (ms < 1) return '<1ms'
  if (ms < 1000) return `${Math.round(ms)}ms`
  return `${(ms / 1000).toFixed(2)}s`
}

export function TraceFilters({
  filters,
  onFilterChange,
  onClear,
  validationWarnings,
  onClearWarnings,
  isLoading,
  searchQuery = '',
  onSearchChange,
  stats,
}: TraceFiltersProps) {
  const [openPopover, setOpenPopover] = useState<PopoverType>(null)
  const [showAttributesPanel, setShowAttributesPanel] = useState(false)

  // Temporary state for text inputs
  const [tempInputs, dispatchTempInputs] = useReducer(tempInputsReducer, {
    tempServiceName: filters.serviceName || '',
    tempOperationName: filters.operationName || '',
    tempMinDuration: filters.minDurationMs?.toString() || '',
    tempMaxDuration: filters.maxDurationMs?.toString() || '',
  })
  const { tempServiceName, tempOperationName, tempMinDuration, tempMaxDuration } = tempInputs

  const [prevFilters, setPrevFilters] = useState({
    serviceName: filters.serviceName,
    operationName: filters.operationName,
    minDurationMs: filters.minDurationMs,
    maxDurationMs: filters.maxDurationMs,
  })

  if (
    prevFilters.serviceName !== filters.serviceName ||
    prevFilters.operationName !== filters.operationName ||
    prevFilters.minDurationMs !== filters.minDurationMs ||
    prevFilters.maxDurationMs !== filters.maxDurationMs
  ) {
    setPrevFilters({
      serviceName: filters.serviceName,
      operationName: filters.operationName,
      minDurationMs: filters.minDurationMs,
      maxDurationMs: filters.maxDurationMs,
    })
    dispatchTempInputs({
      type: 'SYNC_FROM_FILTERS',
      payload: {
        serviceName: filters.serviceName,
        operationName: filters.operationName,
        minDurationMs: filters.minDurationMs,
        maxDurationMs: filters.maxDurationMs,
      },
    })
  }

  // Refs for click-outside detection
  const statusRef = useRef<HTMLDivElement>(null)
  const timeRangeRef = useRef<HTMLDivElement>(null)
  const durationRef = useRef<HTMLDivElement>(null)
  const serviceRef = useRef<HTMLDivElement>(null)
  const operationRef = useRef<HTMLDivElement>(null)
  const sortRef = useRef<HTMLDivElement>(null)

  useClickOutside(statusRef, () => openPopover === 'status' && setOpenPopover(null))
  useClickOutside(timeRangeRef, () => openPopover === 'timeRange' && setOpenPopover(null))
  useClickOutside(durationRef, () => openPopover === 'duration' && setOpenPopover(null))
  useClickOutside(serviceRef, () => openPopover === 'service' && setOpenPopover(null))
  useClickOutside(operationRef, () => openPopover === 'operation' && setOpenPopover(null))
  useClickOutside(sortRef, () => openPopover === 'sort' && setOpenPopover(null))

  const togglePopover = (popover: PopoverType) => {
    setOpenPopover(openPopover === popover ? null : popover)
  }

  const handleStatusChange = (status: 'ok' | 'error' | 'unset' | null) => {
    onFilterChange('status', status)
    setOpenPopover(null)
  }

  const handleTimeRangeChange = (milliseconds: number) => {
    const now = Date.now()
    onFilterChange('startTime', now - milliseconds)
    onFilterChange('endTime', now)
    setOpenPopover(null)
  }

  const handleServiceApply = () => {
    onFilterChange('serviceName', tempServiceName || undefined)
    setOpenPopover(null)
  }

  const handleOperationApply = () => {
    onFilterChange('operationName', tempOperationName || undefined)
    setOpenPopover(null)
  }

  const handleDurationApply = () => {
    const parsedMin = tempMinDuration ? Number.parseInt(tempMinDuration, 10) : null
    const parsedMax = tempMaxDuration ? Number.parseInt(tempMaxDuration, 10) : null
    onFilterChange(
      'minDurationMs',
      parsedMin !== null && Number.isFinite(parsedMin) ? parsedMin : null,
    )
    onFilterChange(
      'maxDurationMs',
      parsedMax !== null && Number.isFinite(parsedMax) ? parsedMax : null,
    )
    setOpenPopover(null)
  }

  const handleSortChange = (sortBy: 'start_time' | 'duration' | 'service_name') => {
    onFilterChange('sortBy', sortBy)
    setOpenPopover(null)
  }

  const handleAttributesChange = (attrs: [string, string][]) => {
    onFilterChange('attributes', attrs.length > 0 ? attrs : undefined)
  }

  const handleKeyDown = (e: React.KeyboardEvent, handler: () => void) => {
    if (e.key === 'Enter') {
      handler()
    }
  }

  const removeFilter = (key: string) => {
    switch (key) {
      case 'status':
        onFilterChange('status', null)
        break
      case 'serviceName':
        onFilterChange('serviceName', undefined)
        dispatchTempInputs({ type: 'CLEAR_SERVICE_NAME' })
        break
      case 'operationName':
        onFilterChange('operationName', undefined)
        dispatchTempInputs({ type: 'CLEAR_OPERATION_NAME' })
        break
      case 'duration':
        onFilterChange('minDurationMs', null)
        onFilterChange('maxDurationMs', null)
        dispatchTempInputs({ type: 'CLEAR_DURATION' })
        break
      case 'timeRange':
        onFilterChange('startTime', null)
        onFilterChange('endTime', null)
        break
      case 'attributes':
        onFilterChange('attributes', undefined)
        break
      case 'sortBy':
        onFilterChange('sortBy', 'start_time')
        break
      case 'sortOrder':
        onFilterChange('sortOrder', 'desc')
        break
    }
  }

  const getStatusLabel = () => {
    if (!filters.status) return 'All'
    return filters.status.charAt(0).toUpperCase() + filters.status.slice(1)
  }

  const getTimeRangeLabel = () => {
    if (!filters.startTime || !filters.endTime) return null
    const diff = filters.endTime - filters.startTime
    const preset = timeRangePresets.find((p) => p.value === diff)
    return preset ? preset.label : 'Custom'
  }

  const getDurationLabel = () => {
    if (!filters.minDurationMs && !filters.maxDurationMs) return null
    const parts = []
    if (filters.minDurationMs) parts.push(`${filters.minDurationMs}ms`)
    if (filters.maxDurationMs) parts.push(`${filters.maxDurationMs}ms`)
    return parts.join(' - ')
  }

  const getSortLabel = () => {
    const option = sortByOptions.find((o) => o.value === filters.sortBy)
    return option ? option.label : 'Start Time'
  }

  // Active filter chips data
  const activeFilters = []
  if (filters.status) {
    activeFilters.push({ key: 'status', label: `Status: ${getStatusLabel()}` })
  }
  if (filters.serviceName) {
    activeFilters.push({ key: 'serviceName', label: `Service: ${filters.serviceName}` })
  }
  if (filters.operationName) {
    activeFilters.push({ key: 'operationName', label: `Operation: ${filters.operationName}` })
  }
  if (getDurationLabel()) {
    activeFilters.push({ key: 'duration', label: `Duration: ${getDurationLabel()}` })
  }
  if (getTimeRangeLabel()) {
    activeFilters.push({ key: 'timeRange', label: getTimeRangeLabel() as string })
  }
  if (filters.attributes && filters.attributes.length > 0) {
    activeFilters.push({ key: 'attributes', label: `Attributes (${filters.attributes.length})` })
  }
  if (filters.sortBy !== 'start_time') {
    activeFilters.push({ key: 'sortBy', label: `Sort: ${getSortLabel()}` })
  }
  if (filters.sortOrder !== 'desc') {
    activeFilters.push({
      key: 'sortOrder',
      label: `Order: ${filters.sortOrder?.toUpperCase() || 'ASC'}`,
    })
  }

  return (
    <div className="space-y-2" data-testid="trace-filters">
      {/* Validation Warnings */}
      {(validationWarnings?.durationSwapped || validationWarnings?.timeRangeSwapped) && (
        <div className="flex items-center justify-between px-3 py-1.5 bg-yellow-950/10 border-l-2 border-yellow-500/50 rounded-sm">
          <div className="flex items-center gap-2 text-yellow-400">
            <AlertTriangle className="w-3.5 h-3.5 flex-shrink-0" />
            <span className="text-[11px]">
              {validationWarnings.durationSwapped && 'Min/max duration swapped. '}
              {validationWarnings.timeRangeSwapped && 'Start/end time swapped. '}
              Values corrected.
            </span>
          </div>
          {onClearWarnings && (
            <button
              type="button"
              onClick={onClearWarnings}
              className="text-yellow-400 hover:text-yellow-300 transition-colors"
              title="Dismiss warning"
            >
              <X className="w-3 h-3" />
            </button>
          )}
        </div>
      )}

      {/* Row 1: Search + Filter Buttons + Stats */}
      <div className="flex items-center gap-2 flex-wrap">
        {/* Inline Search */}
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-muted" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => onSearchChange?.(e.target.value)}
            placeholder="Search traces..."
            className="w-44 pl-7 pr-7 py-1 bg-[#0A0A0A] border border-[#1D1D1D] rounded-md text-[11px] text-foreground placeholder-[#5B5B5B] focus:outline-none focus:border-yellow/50 transition-colors"
          />
          {searchQuery && (
            <button
              type="button"
              onClick={() => onSearchChange?.('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted hover:text-foreground transition-colors"
            >
              <X className="w-3 h-3" />
            </button>
          )}
        </div>

        <div className="w-px h-5 bg-[#1D1D1D]" />

        {/* Status */}
        <div ref={statusRef} className="relative">
          <button
            type="button"
            onClick={() => togglePopover('status')}
            className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
              filters.status
                ? 'bg-yellow/10 border border-yellow/30 text-yellow'
                : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
            }`}
          >
            <CircleDot className="w-3 h-3" />
            Status
          </button>
          {openPopover === 'status' && (
            <div className="absolute top-full mt-1 left-0 bg-[#141414] border border-[#1D1D1D] rounded-md shadow-lg z-20 min-w-[100px]">
              {statusOptions.map((option) => (
                <button
                  key={option.label}
                  type="button"
                  onClick={() =>
                    handleStatusChange(option.value as 'ok' | 'error' | 'unset' | null)
                  }
                  className="w-full px-3 py-1.5 text-left text-[11px] text-muted hover:bg-[#1A1A1A] hover:text-foreground first:rounded-t-md last:rounded-b-md transition-colors"
                >
                  {option.label}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Time Range */}
        <div ref={timeRangeRef} className="relative">
          <button
            type="button"
            onClick={() => togglePopover('timeRange')}
            className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
              filters.startTime && filters.endTime
                ? 'bg-yellow/10 border border-yellow/30 text-yellow'
                : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
            }`}
          >
            <Clock className="w-3 h-3" />
            Time Range
          </button>
          {openPopover === 'timeRange' && (
            <div className="absolute top-full mt-1 left-0 bg-[#141414] border border-[#1D1D1D] rounded-md shadow-lg z-20 min-w-[110px]">
              {timeRangePresets.map((preset) => (
                <button
                  key={preset.label}
                  type="button"
                  onClick={() => handleTimeRangeChange(preset.value)}
                  className="w-full px-3 py-1.5 text-left text-[11px] text-muted hover:bg-[#1A1A1A] hover:text-foreground first:rounded-t-md last:rounded-b-md transition-colors"
                >
                  {preset.label}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Duration */}
        <div ref={durationRef} className="relative">
          <button
            type="button"
            onClick={() => togglePopover('duration')}
            className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
              filters.minDurationMs || filters.maxDurationMs
                ? 'bg-yellow/10 border border-yellow/30 text-yellow'
                : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
            }`}
          >
            <Timer className="w-3 h-3" />
            Duration
          </button>
          {openPopover === 'duration' && (
            <div className="absolute top-full mt-1 left-0 bg-[#141414] border border-[#1D1D1D] rounded-md shadow-lg z-20 p-3 min-w-[200px]">
              <div className="flex items-center gap-2">
                <input
                  type="number"
                  placeholder="Min"
                  value={tempMinDuration}
                  onChange={(e) =>
                    dispatchTempInputs({ type: 'SET_MIN_DURATION', payload: e.target.value })
                  }
                  onKeyDown={(e) => handleKeyDown(e, handleDurationApply)}
                  className="w-20 px-2 py-1 bg-[#0A0A0A] border border-[#1D1D1D] rounded text-[11px] text-foreground placeholder-[#5B5B5B] focus:outline-none focus:border-yellow transition-colors"
                />
                <span className="text-[11px] text-muted">-</span>
                <input
                  type="number"
                  placeholder="Max"
                  value={tempMaxDuration}
                  onChange={(e) =>
                    dispatchTempInputs({ type: 'SET_MAX_DURATION', payload: e.target.value })
                  }
                  onKeyDown={(e) => handleKeyDown(e, handleDurationApply)}
                  className="w-20 px-2 py-1 bg-[#0A0A0A] border border-[#1D1D1D] rounded text-[11px] text-foreground placeholder-[#5B5B5B] focus:outline-none focus:border-yellow transition-colors"
                />
                <span className="text-[11px] text-muted">ms</span>
              </div>
              <button
                type="button"
                onClick={handleDurationApply}
                className="w-full mt-2 px-3 py-1 bg-yellow/10 border border-yellow/30 text-yellow rounded text-[11px] hover:bg-yellow/20 transition-colors"
              >
                Apply
              </button>
            </div>
          )}
        </div>

        {/* Service */}
        <div ref={serviceRef} className="relative">
          <button
            type="button"
            onClick={() => togglePopover('service')}
            className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
              filters.serviceName
                ? 'bg-yellow/10 border border-yellow/30 text-yellow'
                : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
            }`}
          >
            <Server className="w-3 h-3" />
            Service
          </button>
          {openPopover === 'service' && (
            <div className="absolute top-full mt-1 left-0 bg-[#141414] border border-[#1D1D1D] rounded-md shadow-lg z-20 p-3 min-w-[220px]">
              <input
                type="text"
                placeholder="e.g., api-*, backend"
                value={tempServiceName}
                onChange={(e) =>
                  dispatchTempInputs({ type: 'SET_SERVICE_NAME', payload: e.target.value })
                }
                onKeyDown={(e) => handleKeyDown(e, handleServiceApply)}
                className="w-full px-2 py-1.5 bg-[#0A0A0A] border border-[#1D1D1D] rounded text-[11px] text-foreground placeholder-[#5B5B5B] focus:outline-none focus:border-yellow transition-colors"
              />
              <button
                type="button"
                onClick={handleServiceApply}
                className="w-full mt-2 px-3 py-1 bg-yellow/10 border border-yellow/30 text-yellow rounded text-[11px] hover:bg-yellow/20 transition-colors"
              >
                Apply
              </button>
            </div>
          )}
        </div>

        {/* Operation */}
        <div ref={operationRef} className="relative">
          <button
            type="button"
            onClick={() => togglePopover('operation')}
            className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
              filters.operationName
                ? 'bg-yellow/10 border border-yellow/30 text-yellow'
                : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
            }`}
          >
            <Workflow className="w-3 h-3" />
            Operation
          </button>
          {openPopover === 'operation' && (
            <div className="absolute top-full mt-1 left-0 bg-[#141414] border border-[#1D1D1D] rounded-md shadow-lg z-20 p-3 min-w-[220px]">
              <input
                type="text"
                placeholder="e.g., GET *, POST /api/*"
                value={tempOperationName}
                onChange={(e) =>
                  dispatchTempInputs({ type: 'SET_OPERATION_NAME', payload: e.target.value })
                }
                onKeyDown={(e) => handleKeyDown(e, handleOperationApply)}
                className="w-full px-2 py-1.5 bg-[#0A0A0A] border border-[#1D1D1D] rounded text-[11px] text-foreground placeholder-[#5B5B5B] focus:outline-none focus:border-yellow transition-colors"
              />
              <button
                type="button"
                onClick={handleOperationApply}
                className="w-full mt-2 px-3 py-1 bg-yellow/10 border border-yellow/30 text-yellow rounded text-[11px] hover:bg-yellow/20 transition-colors"
              >
                Apply
              </button>
            </div>
          )}
        </div>

        {/* Sort */}
        <div ref={sortRef} className="relative">
          <button
            type="button"
            onClick={() => togglePopover('sort')}
            className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
              filters.sortBy !== 'start_time' || filters.sortOrder !== 'desc'
                ? 'bg-yellow/10 border border-yellow/30 text-yellow'
                : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
            }`}
          >
            <ArrowUpDown className="w-3 h-3" />
            {getSortLabel()} · {filters.sortOrder === 'asc' ? 'Asc' : 'Desc'}
          </button>
          {openPopover === 'sort' && (
            <div className="absolute top-full mt-1 left-0 bg-[#141414] border border-[#1D1D1D] rounded-md shadow-lg z-20 min-w-[140px]">
              <div className="px-3 py-1.5 text-[10px] text-[#5B5B5B] uppercase tracking-wider">
                Sort by
              </div>
              {sortByOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() =>
                    handleSortChange(option.value as 'start_time' | 'duration' | 'service_name')
                  }
                  className={`w-full px-3 py-1.5 text-left text-[11px] hover:bg-[#1A1A1A] transition-colors ${
                    filters.sortBy === option.value
                      ? 'text-yellow'
                      : 'text-muted hover:text-foreground'
                  }`}
                >
                  {option.label}
                </button>
              ))}
              <div className="border-t border-[#1D1D1D] my-1" />
              <div className="px-3 py-1.5 text-[10px] text-[#5B5B5B] uppercase tracking-wider">
                Order
              </div>
              <button
                type="button"
                onClick={() => {
                  onFilterChange('sortOrder', 'asc')
                  setOpenPopover(null)
                }}
                className={`w-full px-3 py-1.5 text-left text-[11px] hover:bg-[#1A1A1A] transition-colors ${
                  filters.sortOrder === 'asc' ? 'text-yellow' : 'text-muted hover:text-foreground'
                }`}
              >
                Ascending
              </button>
              <button
                type="button"
                onClick={() => {
                  onFilterChange('sortOrder', 'desc')
                  setOpenPopover(null)
                }}
                className={`w-full px-3 py-1.5 text-left text-[11px] hover:bg-[#1A1A1A] last:rounded-b-md transition-colors ${
                  filters.sortOrder === 'desc' ? 'text-yellow' : 'text-muted hover:text-foreground'
                }`}
              >
                Descending
              </button>
            </div>
          )}
        </div>

        {/* Attributes */}
        <button
          type="button"
          onClick={() => setShowAttributesPanel(!showAttributesPanel)}
          className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-md transition-colors ${
            filters.attributes && filters.attributes.length > 0
              ? 'bg-yellow/10 border border-yellow/30 text-yellow'
              : 'bg-[#141414] border border-[#1D1D1D] text-muted hover:border-[#2D2D2D] hover:text-foreground'
          }`}
        >
          <Tags className="w-3 h-3" />
          Attributes
        </button>

        {/* Stats (pushed to right) */}
        {stats && (
          <div className="ml-auto flex items-center gap-3 text-[11px] text-muted">
            <div className="flex items-center gap-1">
              <Hash className="w-3 h-3" />
              <span className="font-medium text-foreground tabular-nums">{stats.totalTraces}</span>
            </div>
            {stats.errorCount > 0 && (
              <div className="flex items-center gap-1 text-error">
                <XCircle className="w-3 h-3" />
                <span className="font-medium tabular-nums">{stats.errorCount}</span>
              </div>
            )}
            <div className="flex items-center gap-1">
              <Timer className="w-3 h-3" />
              <span className="font-medium text-foreground tabular-nums">
                {formatDuration(stats.avgDuration)}
              </span>
            </div>
          </div>
        )}
      </div>

      {/* Attributes Panel (Expandable) */}
      {showAttributesPanel && (
        <div className="p-4 bg-[#141414] border border-[#1D1D1D] rounded-lg">
          <AttributesFilter value={filters.attributes || []} onChange={handleAttributesChange} />
        </div>
      )}

      {/* Applied Filter Chips (at the bottom) */}
      {activeFilters.length > 0 && (
        <div className="flex items-center gap-1.5 flex-wrap pt-0.5">
          {isLoading && <div className="w-1.5 h-1.5 bg-yellow rounded-full animate-pulse mr-0.5" />}
          {activeFilters.map((filter) => (
            <button
              key={filter.key}
              type="button"
              onClick={() => removeFilter(filter.key)}
              className="flex items-center gap-1 px-1.5 py-0.5 bg-[#1A1A1A] border border-[#2D2D2D] hover:border-yellow/30 rounded text-[10px] text-muted hover:text-foreground transition-colors group"
            >
              <span>{filter.label}</span>
              <X className="w-2.5 h-2.5 opacity-50 group-hover:opacity-100 group-hover:text-yellow transition-all" />
            </button>
          ))}
          <button
            type="button"
            onClick={onClear}
            className="text-[10px] text-[#5B5B5B] hover:text-foreground transition-colors ml-1"
          >
            Clear all
          </button>
        </div>
      )}
    </div>
  )
}
