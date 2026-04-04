import { ArrowDown, Zap } from 'lucide-react'
import { useMemo } from 'react'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'
import { classifySpanType, formatDuration } from '@/lib/traceUtils'

interface WorkflowChainProps {
  data: WaterfallData
  onSpanClick?: (span: VisualizationSpan) => void
}

interface ChainStep {
  span: VisualizationSpan
  type: 'trigger' | 'enqueue' | 'function'
  label: string
  sublabel?: string
}

const TYPE_COLORS = {
  trigger: {
    bg: 'bg-green-500/10',
    border: 'border-green-500/20',
    text: 'text-green-400',
    dot: 'bg-green-400',
  },
  enqueue: {
    bg: 'bg-yellow/10',
    border: 'border-yellow/20',
    text: 'text-yellow',
    dot: 'bg-yellow',
  },
  function: {
    bg: 'bg-cyan-500/10',
    border: 'border-cyan-500/20',
    text: 'text-cyan-400',
    dot: 'bg-cyan-400',
  },
} as const

export function WorkflowChain({ data, onSpanClick }: WorkflowChainProps) {
  const chain = useMemo(() => {
    const steps: ChainStep[] = []

    for (const span of data.spans) {
      const attrs = span.attributes || {}
      const topic = attrs['messaging.destination.name'] as string | undefined
      const functionId = (attrs['faas.invoked_name'] || attrs.function_id) as string | undefined
      const spanType = classifySpanType(span)

      if (spanType === 'enqueue') {
        steps.push({
          span,
          type: 'enqueue',
          label: topic || 'enqueue',
          sublabel: functionId,
        })
      } else if (spanType === 'trigger') {
        steps.push({
          span,
          type: 'trigger',
          label: functionId || span.name,
          sublabel: formatDuration(span.duration_ms),
        })
      } else if (functionId && span.depth === 0) {
        steps.push({
          span,
          type: 'function',
          label: functionId,
          sublabel: formatDuration(span.duration_ms),
        })
      }
    }

    return steps
  }, [data.spans])

  if (chain.length < 2) return null

  return (
    <div className="px-4 py-3 border-t border-[#1D1D1D]">
      <div className="text-[10px] text-gray-500 uppercase tracking-wider mb-2 flex items-center gap-1.5">
        <Zap className="w-3 h-3" />
        Workflow Chain
      </div>
      <div className="space-y-0">
        {chain.map((step, i) => {
          const colors = TYPE_COLORS[step.type]
          return (
            <div key={step.span.span_id}>
              <button
                type="button"
                onClick={() => onSpanClick?.(step.span)}
                className={`w-full flex items-center gap-2 px-2.5 py-1.5 rounded ${colors.bg} border ${colors.border} hover:brightness-125 transition-all text-left group`}
              >
                <div className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${colors.dot}`} />
                <div className="flex-1 min-w-0">
                  <div className={`text-[11px] font-mono truncate ${colors.text}`}>
                    {step.label}
                  </div>
                  {step.sublabel && (
                    <div className="text-[9px] text-gray-600 truncate">{step.sublabel}</div>
                  )}
                </div>
                <span className="text-[9px] font-mono text-gray-600 flex-shrink-0">
                  {step.type}
                </span>
              </button>
              {i < chain.length - 1 && (
                <div className="flex justify-center py-0.5">
                  <ArrowDown className="w-3 h-3 text-gray-700" />
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
