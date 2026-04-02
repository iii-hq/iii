'use client'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { TracesFilterParams } from '@/api'

export interface TraceFilterState {
  serviceName?: string
  operationName?: string
  status?: 'ok' | 'error' | 'unset' | null
  minDurationMs?: number | null
  maxDurationMs?: number | null
  startTime?: number | null
  endTime?: number | null
  attributes?: [string, string][]
  sortBy?: 'start_time' | 'duration' | 'service_name'
  sortOrder?: 'asc' | 'desc'
  page: number
  pageSize: number
}

const defaultFilters: TraceFilterState = {
  serviceName: undefined,
  operationName: undefined,
  status: null,
  minDurationMs: null,
  maxDurationMs: null,
  startTime: null,
  endTime: null,
  attributes: undefined,
  sortBy: 'start_time',
  sortOrder: 'desc',
  page: 1,
  pageSize: 50,
}

/**
 * Hook for managing trace filter state with server-side pagination.
 *
 * Features:
 * - 300ms debouncing for text inputs (serviceName, operationName)
 * - Automatic page reset when filters change
 * - Type-safe API parameter conversion (camelCase → snake_case)
 * - Range validation (auto-swaps min/max if invalid)
 *
 * @returns {Object} Filter state and control functions
 * @returns {TraceFilterState} filters - Current filter state
 * @returns {Function} updateFilter - Update a single filter field
 * @returns {Function} resetFilters - Reset all filters to defaults
 * @returns {Function} getActiveFilterCount - Count non-default filters
 * @returns {Function} getApiParams - Convert to API-ready parameters
 *
 * @example
 * const { filters, updateFilter, getApiParams } = useTraceFilters()
 *
 * updateFilter('serviceName', 'api-gateway')
 * updateFilter('minDurationMs', 100)
 *
 * const apiParams = getApiParams()
 * // { service_name: "api-gateway", min_duration_ms: 100, offset: 0, limit: 50 }
 */
export function useTraceFilters() {
  const [filters, setFilters] = useState<TraceFilterState>(defaultFilters)
  const [debouncedServiceName, setDebouncedServiceName] = useState<string | undefined>(undefined)
  const [debouncedOperationName, setDebouncedOperationName] = useState<string | undefined>(
    undefined,
  )

  const serviceNameTimerRef = useRef<NodeJS.Timeout | null>(null)
  const operationNameTimerRef = useRef<NodeJS.Timeout | null>(null)

  const [validationWarnings, setValidationWarnings] = useState<{
    durationSwapped?: boolean
    timeRangeSwapped?: boolean
  }>({})

  useEffect(() => {
    return () => {
      if (serviceNameTimerRef.current) clearTimeout(serviceNameTimerRef.current)
      if (operationNameTimerRef.current) clearTimeout(operationNameTimerRef.current)
    }
  }, [])

  /** Update a single filter field. Resets page to 1 when any filter changes (except page/pageSize). */
  const updateFilter = useCallback((key: keyof TraceFilterState, value: unknown) => {
    setFilters((prev) => {
      const newFilters = { ...prev, [key]: value }

      // Reset page to 1 when any filter changes (except page/pageSize)
      if (key !== 'page' && key !== 'pageSize') {
        newFilters.page = 1
      }

      return newFilters
    })

    // Handle debouncing for text inputs
    if (key === 'serviceName') {
      if (serviceNameTimerRef.current) clearTimeout(serviceNameTimerRef.current)
      const timer = setTimeout(() => {
        setDebouncedServiceName(value as string | undefined)
      }, 300)
      serviceNameTimerRef.current = timer
    }

    if (key === 'operationName') {
      if (operationNameTimerRef.current) clearTimeout(operationNameTimerRef.current)
      const timer = setTimeout(() => {
        setDebouncedOperationName(value as string | undefined)
      }, 300)
      operationNameTimerRef.current = timer
    }
  }, [])

  const resetFilters = useCallback(() => {
    setFilters(defaultFilters)
    setDebouncedServiceName(undefined)
    setDebouncedOperationName(undefined)
    if (serviceNameTimerRef.current) clearTimeout(serviceNameTimerRef.current)
    if (operationNameTimerRef.current) clearTimeout(operationNameTimerRef.current)
  }, [])

  const getActiveFilterCount = useCallback(() => {
    let count = 0
    if (filters.serviceName) count++
    if (filters.operationName) count++
    if (filters.status != null) count++
    if (filters.minDurationMs !== null) count++
    if (filters.maxDurationMs !== null) count++
    if (filters.startTime !== null) count++
    if (filters.endTime !== null) count++
    if (filters.attributes && filters.attributes.length > 0) count++
    if (filters.sortBy !== 'start_time') count++
    if (filters.sortOrder !== 'desc') count++
    return count
  }, [filters])

  // Filter-only params (excludes pagination) -- stable reference when only page changes
  const { filterOnlyParams, computedWarnings } = useMemo(() => {
    const params: TracesFilterParams = {}
    const warnings: { durationSwapped?: boolean; timeRangeSwapped?: boolean } = {}

    if (filters.serviceName) params.service_name = filters.serviceName
    if (filters.operationName) {
      params.name = filters.operationName
      params.search_all_spans = true
    }
    if (filters.status != null) params.status = filters.status

    if (filters.minDurationMs != null && filters.maxDurationMs != null) {
      if (filters.minDurationMs > filters.maxDurationMs) {
        params.min_duration_ms = filters.maxDurationMs
        params.max_duration_ms = filters.minDurationMs
        warnings.durationSwapped = true
      } else {
        params.min_duration_ms = filters.minDurationMs
        params.max_duration_ms = filters.maxDurationMs
      }
    } else {
      if (filters.minDurationMs != null) params.min_duration_ms = filters.minDurationMs
      if (filters.maxDurationMs != null) params.max_duration_ms = filters.maxDurationMs
    }

    if (filters.startTime != null && filters.endTime != null) {
      if (filters.startTime > filters.endTime) {
        params.start_time = filters.endTime
        params.end_time = filters.startTime
        warnings.timeRangeSwapped = true
      } else {
        params.start_time = filters.startTime
        params.end_time = filters.endTime
      }
    } else {
      if (filters.startTime != null) params.start_time = filters.startTime
      if (filters.endTime != null) params.end_time = filters.endTime
    }

    if (filters.attributes && filters.attributes.length > 0) params.attributes = filters.attributes
    if (filters.sortBy) params.sort_by = filters.sortBy
    if (filters.sortOrder) params.sort_order = filters.sortOrder

    return { filterOnlyParams: params, computedWarnings: warnings }
  }, [
    filters.serviceName,
    filters.operationName,
    filters.status,
    filters.minDurationMs,
    filters.maxDurationMs,
    filters.startTime,
    filters.endTime,
    filters.attributes,
    filters.sortBy,
    filters.sortOrder,
  ])

  // Full params including pagination offset/limit
  const apiParams = useMemo(
    () => ({
      ...filterOnlyParams,
      offset: (filters.page - 1) * filters.pageSize,
      limit: filters.pageSize,
    }),
    [filterOnlyParams, filters.page, filters.pageSize],
  )

  useEffect(() => {
    setValidationWarnings(computedWarnings)
  }, [computedWarnings])

  const getApiParams = useCallback((): TracesFilterParams => {
    return apiParams
  }, [apiParams])

  // Returns filter params without pagination - stable when only page changes
  const getFilterOnlyParams = useCallback((): TracesFilterParams => {
    return filterOnlyParams
  }, [filterOnlyParams])

  const clearValidationWarnings = useCallback(() => {
    setValidationWarnings({})
  }, [])

  return {
    filters,
    debouncedServiceName,
    debouncedOperationName,
    updateFilter,
    resetFilters,
    getActiveFilterCount,
    getApiParams,
    getFilterOnlyParams,
    validationWarnings,
    clearValidationWarnings,
  }
}
