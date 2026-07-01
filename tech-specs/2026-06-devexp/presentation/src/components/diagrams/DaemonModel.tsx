import { SPAWN_PATHS, SUPERVISION_TIERS } from '@/content/changes'
import { cn } from '@/lib/utils'

/**
 * the "four (really five) detached spawn paths collapse into spawn_owned"
 * funnel — the structural heart of "zero zombies by construction".
 */
export function SpawnFunnel({ className }: { className?: string }) {
  const left = SPAWN_PATHS
  const rowH = 56
  const top = 16
  const leftX = 12
  const leftW = 250
  const ownedX = 470
  const ownedY = 16
  const ownedH = 200
  const deleteY = 240
  const deleteH = 60
  const height = 320
  const cy = (i: number) => top + i * rowH + 22

  return (
    <div className={cn('border border-rule bg-bg', className)}>
      <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
        four detached spawn paths → one reaping parent
      </div>
      <div className="overflow-x-auto p-3">
        <svg viewBox={`0 0 720 ${height}`} className="w-full h-auto min-w-[600px] font-mono select-none" role="img" aria-label="spawn paths collapse into spawn_owned">
          <defs>
            <marker id="fn-arr" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-ink-faint" />
            </marker>
            <marker id="fn-arr-alert" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" className="fill-alert" />
            </marker>
          </defs>

          {/* converging edges */}
          {left.map((p, i) => {
            const del = p.fate === 'delete'
            const targetY = del ? deleteY + deleteH / 2 : ownedY + ownedH / 2
            return (
              <path
                key={p.id}
                d={`M ${leftX + leftW} ${cy(i)} C ${ownedX - 60} ${cy(i)}, ${ownedX - 60} ${targetY}, ${ownedX} ${targetY}`}
                fill="none"
                strokeWidth={1}
                strokeDasharray={del ? '5 4' : undefined}
                markerEnd={`url(#${del ? 'fn-arr-alert' : 'fn-arr'})`}
                className={del ? 'stroke-alert' : 'stroke-rule'}
              />
            )
          })}

          {/* left nodes */}
          {left.map((p, i) => (
            <g key={p.id}>
              <rect x={leftX} y={top + i * rowH} width={leftW} height={44} className="fill-bg stroke-rule" strokeWidth={1} />
              <text x={leftX + 12} y={top + i * rowH + 18} fontSize="11.5" fontWeight={600} className="fill-ink">
                {p.id} · {p.label}
              </text>
              <text x={leftX + 12} y={top + i * rowH + 33} fontSize="9.5" className="fill-ink-ghost">
                {p.detach}
              </text>
            </g>
          ))}

          {/* spawn_owned target */}
          <rect x={ownedX} y={ownedY} width={238} height={ownedH} className="fill-panel stroke-accent" strokeWidth={1.5} />
          <rect x={ownedX} y={ownedY} width={3} height={ownedH} className="fill-accent" />
          <text x={ownedX + 18} y={ownedY + 30} fontSize="14" fontWeight={600} className="fill-ink">spawn_owned</text>
          {[
            'setpgid(0,0) — own group,',
            'still the daemon’s child',
            'keep the Child forever',
            'Stdio::piped — logs captured',
            'instance_token injected',
            'pidfd / kqueue exit-watch',
            '→ wait()-reaping is automatic',
          ].map((line, i) => (
            <text key={i} x={ownedX + 18} y={ownedY + 54 + i * 19} fontSize="10.5" className={i === 6 ? 'fill-accent' : 'fill-ink-faint'}>
              {line}
            </text>
          ))}

          {/* delete target */}
          <rect x={ownedX} y={deleteY} width={238} height={deleteH} className="fill-bg stroke-alert" strokeWidth={1} strokeDasharray="5 4" />
          <text x={ownedX + 18} y={deleteY + 25} fontSize="13" fontWeight={600} className="fill-alert">path E — DELETED</text>
          <text x={ownedX + 18} y={deleteY + 43} fontSize="9.5" className="fill-ink-faint">ops calls process::start over ws</text>
        </svg>
      </div>
    </div>
  )
}

/** the supervision tiers — the daemon is parented by the OS, never the engine */
export function SupervisionTiers({ className }: { className?: string }) {
  return (
    <div className={cn('flex flex-col', className)}>
      {SUPERVISION_TIERS.map((t, i) => (
        <div key={t.tier} className="relative">
          <div
            className={cn(
              'border border-rule bg-bg p-4',
              i === 1 && 'border-l-2 border-l-accent bg-panel',
            )}
          >
            <div className="flex items-baseline gap-x-3">
              <span className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-ghost tabular-nums shrink-0">
                {t.tier}
              </span>
              <span className="font-mono text-[15px] font-semibold lowercase text-ink">
                {t.name}
              </span>
            </div>
            <p className="mt-2 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
              {t.body}
            </p>
          </div>
          {i < SUPERVISION_TIERS.length - 1 ? (
            <div className="flex items-center justify-center py-1.5">
              <span className="font-mono text-[11px] text-ink-ghost">↓ owns + restarts</span>
            </div>
          ) : null}
        </div>
      ))}
    </div>
  )
}
