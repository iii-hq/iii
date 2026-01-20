import { Badge, type BadgeProps } from '@motiadev/ui'
import type React from 'react'
import { memo } from 'react'
import type { TraceGroup } from '@/types/observability'

type Props = {
  status: TraceGroup['status']
  duration?: string
}

const variantMap = {
  running: 'info',
  completed: 'success',
  failed: 'error',
  default: 'default',
}

export const TraceStatusBadge: React.FC<Props> = memo(({ status, duration }) => (
  <Badge variant={variantMap[status] as BadgeProps['variant']}>
    {duration && status !== 'failed' ? duration : status}
  </Badge>
))

TraceStatusBadge.displayName = 'TraceStatusBadge'
