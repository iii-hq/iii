/**
 * Span transformation utilities for visualization
 * Converts StoredSpan[] to visualization-ready format with computed depth, positioning
 */

import type { SpanTreeNode, StoredSpan } from '@/api'

/**
 * Visualization-ready span with computed positioning
 */
export interface VisualizationSpan {
  name: string
  span_id: string
  parent_span_id?: string
  trace_id: string
  duration_ms: number
  status: 'ok' | 'error' | 'unset'
  depth: number
  start_percent: number
  width_percent: number
  attributes: Record<string, unknown>
  events: StoredSpan['events']
  links: StoredSpan['links']
  kind?: string
  service_name?: string
  instrumentation_scope_name?: string
  instrumentation_scope_version?: string
  flags?: number
}

/**
 * Waterfall visualization data
 */
export interface WaterfallData {
  spans: VisualizationSpan[]
  total_duration_ms: number
  span_count: number
}

/**
 * Calculate span depth in the tree
 */
function calculateDepths(spans: StoredSpan[]): Map<string, number> {
  const depths = new Map<string, number>()
  const spanMap = new Map(spans.map((s) => [s.span_id, s]))

  function getDepth(span: StoredSpan): number {
    if (depths.has(span.span_id)) {
      return depths.get(span.span_id) ?? 0
    }

    if (!span.parent_span_id || !spanMap.has(span.parent_span_id)) {
      depths.set(span.span_id, 0)
      return 0
    }

    const parentSpan = spanMap.get(span.parent_span_id)
    const parentDepth = parentSpan ? getDepth(parentSpan) : 0
    const depth = parentDepth + 1
    depths.set(span.span_id, depth)
    return depth
  }

  for (const span of spans) {
    getDepth(span)
  }

  return depths
}

/**
 * Threshold for detecting nanosecond timestamps (Jan 1, 2100 in milliseconds)
 */
const NANO_THRESHOLD = 4102444800000

/**
 * Convert timestamp to milliseconds
 * Auto-detects nanosecond vs millisecond timestamps
 */
export function toMs(timestamp: number): number {
  if (!Number.isFinite(timestamp)) return 0
  return timestamp > NANO_THRESHOLD ? timestamp / 1_000_000 : timestamp
}

/**
 * Calculate duration in milliseconds between two timestamps
 * Handles both nanosecond and millisecond timestamps
 */
export function calculateDurationMs(startTime: number, endTime: number): number {
  const startMs = toMs(startTime)
  const endMs = toMs(endTime)
  const duration = endMs - startMs
  return Number.isFinite(duration) && duration >= 0 ? duration : 0
}

/**
 * Get span status from status string
 */
function getSpanStatus(status: StoredSpan['status']): 'ok' | 'error' | 'unset' {
  if (!status) return 'unset'
  const lower = status.toLowerCase()
  if (lower === 'error' || lower === '2') return 'error'
  if (lower === 'ok' || lower === '1') return 'ok'
  if (lower === 'unset' || lower === '0') return 'unset'
  return 'unset'
}

/**
 * Convert attributes from array-of-tuples to Record.
 * Handles both `[["key","val"], ...]` (engine format) and already-converted Records.
 */
function attributesToRecord(
  attributes: Array<[string, unknown]> | Record<string, unknown> | undefined,
): Record<string, unknown> {
  if (!attributes) return Object.create(null) as Record<string, unknown>

  const record: Record<string, unknown> = Object.create(null)

  if (!Array.isArray(attributes)) {
    for (const [key, value] of Object.entries(attributes)) {
      record[key] = value
    }
    return record
  }

  for (const item of attributes) {
    if (Array.isArray(item) && item.length >= 2) {
      record[String(item[0])] = item[1]
    }
  }
  return record
}

/**
 * Transform raw spans for a specific trace into WaterfallData
 * @param spans - All spans (will be filtered by traceId)
 * @param traceId - Trace ID to filter spans
 * @returns WaterfallData with computed positions and depths
 */
export function toWaterfallData(spans: StoredSpan[], traceId: string): WaterfallData | null {
  const traceSpans = spans.filter((s) => s.trace_id === traceId)

  if (traceSpans.length === 0) {
    return null
  }

  // Calculate trace boundaries (in milliseconds)
  const minStart = Math.min(...traceSpans.map((s) => toMs(s.start_time_unix_nano)))
  const maxEnd = Math.max(...traceSpans.map((s) => toMs(s.end_time_unix_nano)))
  const totalDurationMs = maxEnd - minStart

  // Calculate depths
  const depths = calculateDepths(traceSpans)
  const spanMap = new Map(traceSpans.map((s) => [s.span_id, s]))

  // Convert to VisualizationSpan format with percentages
  const visualSpans: VisualizationSpan[] = traceSpans.map((storedSpan) => {
    const durationMs = calculateDurationMs(
      storedSpan.start_time_unix_nano,
      storedSpan.end_time_unix_nano,
    )
    const startOffset = toMs(storedSpan.start_time_unix_nano) - minStart
    const startPercent = totalDurationMs > 0 ? (startOffset / totalDurationMs) * 100 : 0
    const widthPercent = totalDurationMs > 0 ? (durationMs / totalDurationMs) * 100 : 100

    return {
      name: storedSpan.name,
      span_id: storedSpan.span_id,
      parent_span_id: storedSpan.parent_span_id,
      trace_id: storedSpan.trace_id,
      duration_ms: durationMs,
      status: getSpanStatus(storedSpan.status),
      depth: depths.get(storedSpan.span_id) || 0,
      start_percent: startPercent,
      width_percent: widthPercent,
      attributes: attributesToRecord(storedSpan.attributes),
      events: (storedSpan.events || []).map((e) => ({
        ...e,
        attributes: attributesToRecord(e.attributes),
      })),
      links: storedSpan.links || [],
      kind: storedSpan.kind,
      service_name:
        storedSpan.service_name || (storedSpan.resource?.['service.name'] as string) || undefined,
      instrumentation_scope_name: undefined,
      instrumentation_scope_version: undefined,
      flags: storedSpan.flags,
    }
  })

  // Sort by start time, then by depth
  visualSpans.sort((a, b) => {
    const aStart = toMs(spanMap.get(a.span_id)?.start_time_unix_nano ?? 0)
    const bStart = toMs(spanMap.get(b.span_id)?.start_time_unix_nano ?? 0)
    if (aStart !== bStart) return aStart - bStart
    return a.depth - b.depth
  })

  return {
    spans: visualSpans,
    total_duration_ms: totalDurationMs,
    span_count: visualSpans.length,
  }
}

/**
 * Flatten a SpanTreeNode tree into a flat list of StoredSpan-like objects
 * Depth is computed naturally from tree nesting level
 */
function flattenTree(
  nodes: SpanTreeNode[],
  depth: number = 0,
): Array<{ span: SpanTreeNode; depth: number }> {
  const result: Array<{ span: SpanTreeNode; depth: number }> = []
  for (const node of nodes) {
    result.push({ span: node, depth })
    if (node.children && node.children.length > 0) {
      result.push(...flattenTree(node.children, depth + 1))
    }
  }
  return result
}

/**
 * Transform a trace tree response into WaterfallData
 * Uses the tree structure to compute depth naturally instead of calculating from parent references
 * @param roots - Root span tree nodes from the trace tree API
 * @returns WaterfallData with computed positions and depths
 */
export function treeToWaterfallData(roots: SpanTreeNode[]): WaterfallData | null {
  if (!roots || roots.length === 0) {
    return null
  }

  // Flatten the tree with depth information
  const flatSpans = flattenTree(roots)

  if (flatSpans.length === 0) {
    return null
  }

  // Calculate trace boundaries (in milliseconds)
  const minStart = Math.min(...flatSpans.map((s) => toMs(s.span.start_time_unix_nano)))
  const maxEnd = Math.max(...flatSpans.map((s) => toMs(s.span.end_time_unix_nano)))
  const totalDurationMs = maxEnd - minStart

  // Convert to VisualizationSpan format
  const visualSpans: VisualizationSpan[] = flatSpans.map(({ span, depth }) => {
    const durationMs = calculateDurationMs(span.start_time_unix_nano, span.end_time_unix_nano)
    const startOffset = toMs(span.start_time_unix_nano) - minStart
    const startPercent = totalDurationMs > 0 ? (startOffset / totalDurationMs) * 100 : 0
    const widthPercent = totalDurationMs > 0 ? (durationMs / totalDurationMs) * 100 : 100

    return {
      name: span.name,
      span_id: span.span_id,
      parent_span_id: span.parent_span_id,
      trace_id: span.trace_id,
      duration_ms: durationMs,
      status: getSpanStatus(span.status),
      depth,
      start_percent: startPercent,
      width_percent: widthPercent,
      attributes: attributesToRecord(span.attributes || []),
      events: (span.events || []).map((e) => ({
        ...e,
        attributes: attributesToRecord(e.attributes),
      })),
      links: span.links || [],
      kind: span.kind,
      service_name: span.service_name || (span.resource?.['service.name'] as string) || undefined,
      instrumentation_scope_name: undefined,
      instrumentation_scope_version: undefined,
      flags: span.flags,
    }
  })

  return {
    spans: visualSpans,
    total_duration_ms: totalDurationMs,
    span_count: visualSpans.length,
  }
}
