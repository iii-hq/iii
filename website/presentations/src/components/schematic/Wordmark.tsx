import { cn } from '@lib/lib/utils'

/**
 * the iii wordmark: three "i"s, each a stem + a tittle, six rectangles total,
 * all sharing the same square unit, all in ink.
 */
export function Wordmark({ className }: { className?: string }) {
  const unit = 4
  const stems = [0, 8, 16]
  return (
    <svg aria-label="iii" role="img" viewBox="0 0 20 18" className={cn('h-[18px] w-auto', className)}>
      {stems.map((x) => (
        <g key={x} className="fill-ink">
          <rect x={x} y={0} width={unit} height={unit} />
          <rect x={x} y={6} width={unit} height={12} />
        </g>
      ))}
    </svg>
  )
}
