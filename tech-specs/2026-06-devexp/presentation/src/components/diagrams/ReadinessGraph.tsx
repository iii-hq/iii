import { Button } from '@/components/schematic/Button'
import { StatusDot } from '@/components/schematic/StatusDot'
import { READINESS_NODES } from '@/content/changes'
import { useStepper } from '@/hooks/useStepper'
import { cn } from '@/lib/utils'

/**
 * dependency-ordered bring-up: configuration (always-ready root) → state, http
 * → math-worker → caller-worker, each gated on L1 readiness. The frontier
 * advances one node at a time; a node is only reached once its deps are ready.
 */
const ORDER = READINESS_NODES // already in topo order

export function ReadinessGraph({ className }: { className?: string }) {
  const stepper = useStepper(ORDER.length, 1500)
  const frontier = stepper.step // index of the node currently coming up

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
        bring-up order — gated on readiness
      </div>

      <div className="p-4 flex flex-col gap-y-2">
        {ORDER.map((n, i) => {
          const ready = i < frontier
          const coming = i === frontier
          return (
            <div key={n.id} className="flex items-center gap-x-3">
              <span className="font-mono text-[11px] text-ink-ghost tabular-nums w-5 shrink-0">
                {String(i).padStart(2, '0')}
              </span>
              <div
                className={cn(
                  'flex-1 border bg-bg px-3.5 py-2.5 transition-colors',
                  ready
                    ? 'border-rule'
                    : coming
                      ? 'border-l-2 border-l-accent border-rule bg-panel'
                      : 'border-rule-2 opacity-60',
                )}
              >
                <div className="flex items-center justify-between gap-x-3">
                  <span className="flex items-center gap-x-2 min-w-0">
                    <span className="font-mono text-[14px] font-semibold lowercase text-ink truncate">
                      {n.label}
                    </span>
                    {'root' in n && n.root ? (
                      <span className="font-mono text-[9px] uppercase tracking-[0.1em] text-ink-ghost border border-rule-2 px-1 py-0.5">
                        always-ready root
                      </span>
                    ) : null}
                  </span>
                  <span className="flex items-center gap-x-1.5 shrink-0">
                    {ready ? (
                      <>
                        <StatusDot tone="accent" />
                        <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-accent">ready</span>
                      </>
                    ) : coming ? (
                      <>
                        <StatusDot tone="accent" pulse />
                        <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-ink-faint">awaiting L1</span>
                      </>
                    ) : (
                      <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-ink-ghost">queued</span>
                    )}
                  </span>
                </div>
                <div className="mt-0.5 font-mono text-[11px] leading-[1.5] text-ink-ghost lowercase">
                  {n.note}
                </div>
              </div>
            </div>
          )
        })}
      </div>

      <div className="flex flex-wrap items-center gap-x-2 gap-y-2 border-t border-rule bg-panel px-3.5 py-2.5">
        <Button variant="pill" size="sm" onClick={stepper.toggle}>
          {stepper.playing ? 'pause' : stepper.atEnd ? 'replay' : 'bring up'}
        </Button>
        <Button variant="ghost" size="sm" onClick={stepper.next} disabled={stepper.atEnd}>
          next →
        </Button>
        <Button variant="ghost" size="sm" onClick={stepper.reset}>
          reset
        </Button>
        <span className="ml-auto font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint tabular-nums">
          {Math.min(frontier, ORDER.length)} / {ORDER.length} ready
        </span>
      </div>
    </div>
  )
}
