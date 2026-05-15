import type { VisualizationSpan } from './traceTransform'

const ENGINE_SERVICE_NAME = 'iii'
const ENGINE_VERB_PREFIXES = ['handle_invocation ', 'call '] as const

export interface SpanKindIndicator {
  icon: string
  label: string
}

export function getSpanKindIndicator(kind: string | undefined): SpanKindIndicator | null {
  if (!kind) return null
  const k = kind.toLowerCase()
  switch (k) {
    case 'server':
      return { icon: '▶', label: 'server (handles incoming)' }
    case 'client':
      return { icon: '↗', label: 'client (outgoing call)' }
    case 'producer':
      return { icon: '↥', label: 'producer (sends to queue)' }
    case 'consumer':
      return { icon: '↧', label: 'consumer (reads from queue)' }
    case 'internal':
      return { icon: '•', label: 'internal' }
    default:
      return null
  }
}

export function formatSpanLabel(span: Pick<VisualizationSpan, 'name' | 'service_name'>): string {
  let label = span.name
  for (const prefix of ENGINE_VERB_PREFIXES) {
    if (label.startsWith(prefix)) {
      label = label.slice(prefix.length)
      break
    }
  }
  if (span.service_name) {
    const servicePrefix = `${span.service_name}.`
    if (label.startsWith(servicePrefix)) {
      label = label.slice(servicePrefix.length)
    }
  }
  return label
}

export function isEngineRoutingSpan(
  span: Pick<VisualizationSpan, 'name' | 'service_name'>,
): boolean {
  if (span.service_name !== ENGINE_SERVICE_NAME) return false
  return ENGINE_VERB_PREFIXES.some((p) => span.name.startsWith(p))
}

export function isEngineRoutingPair(
  parent: Pick<VisualizationSpan, 'name' | 'service_name'>,
  child: Pick<VisualizationSpan, 'name' | 'service_name'>,
): boolean {
  if (parent.service_name !== ENGINE_SERVICE_NAME) return false
  if (child.service_name !== ENGINE_SERVICE_NAME) return false
  if (!parent.name.startsWith('handle_invocation ')) return false
  if (!child.name.startsWith('call ')) return false
  return parent.name.slice('handle_invocation '.length) === child.name.slice('call '.length)
}
