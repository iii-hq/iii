import { Badge, type BadgeProps } from '@motiadev/ui'
import type React from 'react'

const map: Record<string, BadgeProps['variant']> = {
  info: 'info',
  error: 'error',
  warn: 'warning',
  debug: 'info',
}

export const LogLevelBadge: React.FC<{ level: string; className?: string }> = (props) => {
  return (
    <Badge variant={map[props.level] as BadgeProps['variant']} className={props.className}>
      {props.level}
    </Badge>
  )
}
