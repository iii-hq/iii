'use client'

import type { ProfileView } from '@/lib/benchmarks/views'
import { cn } from '@/lib/utils'

import { useProfile } from './profile'

export { LineChart as Chart } from './line-chart'

/**
 * One number per machine size, in a small table beside a section's text. The selected size's row is lit;
 * clicking a row selects that size.
 */
export function SizeTable({
  columns,
  rows,
  className,
}: {
  columns: string[]
  rows: { profile: ProfileView; cells: string[] }[]
  className?: string
}) {
  const { selected, select } = useProfile()
  return (
    <table className={cn('w-full border-separate border-spacing-0 text-[13px]', className)}>
      <thead>
        <tr>
          <th scope="col" className="pb-2 text-left font-normal text-muted-foreground">
            Size
          </th>
          {columns.map((c) => (
            <th
              key={c}
              scope="col"
              className="whitespace-nowrap pb-2 pl-3 text-right font-normal text-muted-foreground"
            >
              {c}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map(({ profile, cells }) => {
          const on = profile.name === selected
          return (
            <tr
              key={profile.name}
              onClick={() => select(profile.name)}
              className={cn(
                'cursor-pointer transition-colors duration-150',
                on ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
              )}
            >
              <th scope="row" className={cn('border-t py-2 pr-3 text-left font-normal', on && 'font-medium')}>
                <span className="inline-flex items-center gap-2">
                  <span
                    aria-hidden
                    className={cn('size-1.5 rounded-full transition-colors', on ? 'bg-foreground' : 'bg-border')}
                  />
                  {profile.label}
                </span>
              </th>
              {cells.map((c, i) => (
                <td
                  // biome-ignore lint/suspicious/noArrayIndexKey: Cells are positional, matching the columns.
                  key={i}
                  className="whitespace-nowrap border-t py-2 pl-3 text-right font-mono tabular-nums"
                >
                  {c}
                </td>
              ))}
            </tr>
          )
        })}
      </tbody>
    </table>
  )
}
