import { memo } from 'react'

type ClosingParenthesisProps = {
  callsQuantity?: number
  noQuantity?: boolean
}

export const ClosingParenthesis: React.FC<ClosingParenthesisProps> = memo(({ callsQuantity, noQuantity = false }) => {
  const hasCalls = callsQuantity && callsQuantity > 1

  return (
    <div className="flex items-center">
      <span className="font-mono text-emerald-500">)</span>
      {!noQuantity && hasCalls && <span className="font-mono text-muted-foreground"> x{callsQuantity}</span>}
    </div>
  )
})
ClosingParenthesis.displayName = 'ClosingParenthesis'
