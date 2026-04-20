import { Position } from '@xyflow/react'
import { clsx } from 'clsx'
import { Children } from 'react'
import type { NodeData } from '../../../api/flows/types'
import { BaseHandle } from './base-handle'
import { NodeHeader } from './node-header'

type Props = {
  title: string
  subtitle?: string
  variant: 'event' | 'http' | 'noop' | 'cron' | 'durable:subscriber' | 'state'
  disableSourceHandle?: boolean
  disableTargetHandle?: boolean
  data: NodeData
  children?: React.ReactNode
}

export function BaseNode({
  title,
  variant,
  children,
  disableSourceHandle,
  disableTargetHandle,
  subtitle,
  data,
}: Props) {
  const sourcePos =
    data.nodeConfig?.sourceHandlePosition === 'right' ? Position.Right : Position.Bottom
  const targetPos = data.nodeConfig?.targetHandlePosition === 'left' ? Position.Left : Position.Top
  const hasChildren = Children.toArray(children).length > 0

  return (
    <div className="rounded-lg max-w-[350px]">
      <div className="rounded-lg bg-[#141414] border border-[#1D1D1D]">
        <div className="group relative">
          <NodeHeader text={title} variant={variant} triggers={data.triggers} />

          {subtitle && <div className="py-3 px-3 text-[11px] text-[#9CA3AF]">{subtitle}</div>}
          {hasChildren && (
            <div className="p-2">
              <div
                className={clsx(
                  'space-y-2 p-3 text-[11px] text-[#9CA3AF]',
                  variant !== 'noop' && 'bg-[#0A0A0A] rounded',
                )}
              >
                {children}
              </div>
            </div>
          )}

          {!disableTargetHandle && <BaseHandle type="target" position={targetPos} />}
          {!disableSourceHandle && <BaseHandle type="source" position={sourcePos} />}
        </div>
      </div>
    </div>
  )
}
