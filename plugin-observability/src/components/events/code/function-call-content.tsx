import { cn } from '@motiadev/ui'
import { forwardRef } from 'react'

type FunctionCallContentProps = {
  topLevelClassName?: string
  objectName?: string
  functionName: string
  className?: string
}

export const FunctionCallContent = forwardRef<HTMLDivElement, FunctionCallContentProps>(
  ({ topLevelClassName, objectName, functionName, className, ...props }, ref) => {
    return (
      <div ref={ref} className={cn('flex items-center', className)} {...props}>
        {topLevelClassName && (
          <>
            <span className="font-mono text-pink-500">{topLevelClassName}</span>
            <span>.</span>
          </>
        )}
        {objectName && (
          <>
            <span className="font-mono text-pink-500">{objectName}</span>
            <span>.</span>
          </>
        )}
        <span className="font-mono text-pink-500">{functionName}</span>
        <span className="font-mono text-emerald-500">(</span>
      </div>
    )
  },
)
FunctionCallContent.displayName = 'FunctionCallContent'
