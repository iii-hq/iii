'use client'

import { ChevronDownIcon } from 'lucide-react'

import { joined, nanos } from '@/lib/benchmarks/format'
import { cn } from '@/lib/utils'

import { Sparkline } from './sparkline'

export interface MicroRowView {
  name: string
  latest: string
  change: number | null
  /** The time at each release it was measured, oldest first, with the release beside it. */
  history: number[]
  releases: string[]
}

export interface MicroGroupView {
  id: string
  title: string
  description: string
  rows: MicroRowView[]
}

/** A change between two releases, in words. Under 10% is within what a shared runner varies by. */
function Change({ change }: { change: number | null }) {
  if (change === null) return <span className="text-muted-foreground">new</span>
  const pct = Math.round(Math.abs(change) * 100)
  if (pct < 10) return <span className="text-muted-foreground">±{pct}%</span>
  return change < 0 ? <span className="text-ok">{pct}% faster</span> : <span className="text-warn">{pct}% slower</span>
}

function Rows({ rows }: { rows: MicroRowView[] }) {
  return (
    <ul>
      {rows.map((r) => (
        <li
          key={r.name}
          className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-1 border-t px-5 py-3 first:border-t-0 sm:grid-cols-[minmax(0,1fr)_140px_88px_92px] sm:px-6"
        >
          <span className="truncate font-mono text-[12.5px] text-foreground">{r.name}</span>
          <Sparkline
            values={r.history}
            labels={r.releases}
            format={(v) => joined(nanos(v))}
            className="col-span-2 row-start-2 sm:col-span-1 sm:row-start-auto"
          />
          <span className="text-right font-mono text-[12.5px] text-foreground tabular-nums sm:order-none">
            {r.latest}
          </span>
          <span className="hidden text-right text-[12.5px] tabular-nums sm:block">
            <Change change={r.change} />
          </span>
        </li>
      ))}
    </ul>
  )
}

/** A group of criterion benchmarks: what it times, then one row per benchmark. */
export function MicroGroupCard({ group }: { group: MicroGroupView }) {
  return (
    <section className="overflow-hidden rounded-xl border bg-card shadow-xs" aria-labelledby={`micro-${group.id}`}>
      <header className="px-5 pt-5 pb-4 sm:px-6">
        <h3 id={`micro-${group.id}`} className="font-medium text-[15px] text-foreground">
          {group.title}
        </h3>
        <p className="mt-1 text-pretty text-[13px] text-muted-foreground leading-snug">{group.description}</p>
      </header>
      <div className="border-t">
        <Rows rows={group.rows} />
      </div>
    </section>
  )
}

/** The rest of the benchmarks, folded: a disclosure per group. */
export function MicroMore({ groups }: { groups: MicroGroupView[] }) {
  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      {groups.map((g) => (
        <details key={g.id} className="group border-t first:border-t-0">
          <summary
            className={cn(
              'flex cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 outline-none sm:px-6',
              'hover:bg-muted/40 focus-visible:bg-muted/40 [&::-webkit-details-marker]:hidden',
            )}
          >
            <span>
              <span className="font-medium text-[14px] text-foreground">{g.title}</span>
              <span className="ml-2 text-[13px] text-muted-foreground">
                {g.rows.length} benchmark{g.rows.length === 1 ? '' : 's'}
              </span>
            </span>
            <ChevronDownIcon
              aria-hidden
              className="size-4 text-muted-foreground transition-transform duration-200 group-open:rotate-180"
            />
          </summary>
          <p className="px-5 pb-3 text-pretty text-[13px] text-muted-foreground sm:px-6">{g.description}</p>
          <div className="border-t">
            <Rows rows={g.rows} />
          </div>
        </details>
      ))}
    </div>
  )
}
