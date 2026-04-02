/**
 * Service color palette for span visualization
 * Colors follow industry-standard observability practices
 */
export const SERVICE_COLORS = [
  '#632CA6', // Purple - Primary service color
  '#4A90D9', // Blue - HTTP/Web services
  '#27AE60', // Green - Cache services
  '#E67E22', // Orange - Queue/Messaging
  '#16A085', // Teal - External APIs
  '#E91E63', // Pink - Auth services
  '#5C6BC0', // Indigo - gRPC/Internal
  '#FFA000', // Amber - Workers
  '#00BCD4', // Cyan - Microservices
  '#E53935', // Red - Critical services
] as const

/**
 * Span status colors (industry standard)
 */
export const SPAN_STATUS_COLORS = {
  ok: '#3FB950', // Green
  error: '#F85149', // Red
  unset: '#6E7681', // Gray
} as const

/**
 * Simple hash function for consistent color assignment
 */
function simpleHash(str: string): number {
  let hash = 0
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i)
    hash = (hash << 5) - hash + char
    hash = hash & hash // Convert to 32-bit integer
  }
  return Math.abs(hash)
}

/**
 * Get a consistent color for a service name
 */
export function getServiceColor(serviceName: string): string {
  if (!serviceName) return SERVICE_COLORS[0]

  const hash = simpleHash(serviceName)
  const index = hash % SERVICE_COLORS.length
  return SERVICE_COLORS[index]
}

/**
 * Get color for span status
 */
export function getSpanStatusColor(status: 'ok' | 'error' | 'unset' = 'unset'): string {
  return SPAN_STATUS_COLORS[status] || SPAN_STATUS_COLORS.unset
}
