import { cn } from '@motiadev/ui'
import type * as React from 'react'
import { memo } from 'react'
import type { ApiRouteMethod } from '../types/endpoint'

const getMethodStyles = (method: ApiRouteMethod): string => {
  switch (method.toUpperCase()) {
    case 'POST':
      return 'bg-[#258DC3]/15 text-[#258DC3]'
    case 'GET':
      return 'bg-[#709A2D]/15 text-[#709A2D]'
    case 'DELETE':
      return 'bg-[#DE2134]/15 text-[#DE2134]'
    case 'PUT':
      return 'bg-[#B9922D]/15 text-[#B9922D]'
    case 'PATCH':
      return 'bg-[#B9922D]/15 text-[#B9922D]'
    case 'HEAD':
      return 'bg-[#E221DF]/15 text-[#E221DF]'
    case 'OPTIONS':
      return 'bg-[#B9922D]/15 text-[#B9922D]'
    default:
      return 'bg-[#258DC3]/15 text-[#258DC3]' // default to GET
  }
}

interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant: ApiRouteMethod
}

export const EndpointBadge = memo(({ className, variant, ...props }: BadgeProps) => {
  const baseClasses = 'rounded-lg px-2 py-0.5 text-xs font-mono font-bold transition-colors'
  const methodClasses = getMethodStyles(variant)

  return <div className={cn(baseClasses, methodClasses, className)} {...props} />
})
