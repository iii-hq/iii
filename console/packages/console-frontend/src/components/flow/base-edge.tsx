import {
  EdgeLabelRenderer,
  type EdgeProps,
  getSmoothStepPath,
  BaseEdge as RFBaseEdge,
} from '@xyflow/react'
import { clsx } from 'clsx'

export function FlowEdge(props: EdgeProps) {
  const { sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, data } = props
  const label = data?.label as string | undefined
  const labelVariant = data?.labelVariant as 'default' | 'conditional' | undefined

  const [edgePath, labelX, labelY] = getSmoothStepPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    borderRadius: 20,
    offset: 10,
  })

  return (
    <>
      <RFBaseEdge
        path={edgePath}
        style={{
          stroke: data?.variant === 'virtual' ? '#9CA3AF' : '#F3F724',
          strokeWidth: 1.5,
          shapeRendering: 'geometricPrecision',
          fill: 'none',
        }}
      />
      {label && (
        <EdgeLabelRenderer>
          <div
            className={clsx(
              'absolute pointer-events-all text-[10px] border px-1.5 py-0.5 font-mono',
              labelVariant === 'conditional'
                ? 'bg-amber-300 border-amber-950 text-amber-950 font-semibold italic rounded-lg'
                : 'border-[#1D1D1D] bg-[#0A0A0A] text-[#9CA3AF] rounded-full',
            )}
            style={{
              transform: `translateX(-50%) translateY(-50%) translate(${labelX}px, ${labelY}px)`,
            }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  )
}
