'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { easeOut } from '@/lib/motion'

import { cn } from '@/lib/utils'
import { useGraphicLoop } from './use-graphic-loop'

type Entry = { id: string; kind: 'fn' | 'trigger' }
type Install = {
  worker: string
  version: string
  entries: Entry[]
  caller: string
  call: string
  result: string
}

const INSTALLS: Install[] = [
  {
    worker: 'database',
    version: '0.8.3',
    entries: [
      { id: 'database::query', kind: 'fn' },
      { id: 'database::execute', kind: 'fn' },
      { id: 'database::transaction', kind: 'fn' },
      { id: 'database:row-changed', kind: 'trigger' },
    ],
    caller: 'agent · next step',
    call: 'database::query',
    result: '12 rows',
  },
  {
    worker: 'browser',
    version: '0.4.1',
    entries: [
      { id: 'browser::navigate', kind: 'fn' },
      { id: 'browser::screenshot', kind: 'fn' },
      { id: 'browser::click', kind: 'fn' },
    ],
    caller: 'agent · next step',
    call: 'browser::navigate',
    result: '200 OK',
  },
  {
    worker: 'slack',
    version: '0.3.2',
    entries: [
      { id: 'slack::post-message', kind: 'fn' },
      { id: 'slack::reply', kind: 'fn' },
      { id: 'slack:message', kind: 'trigger' },
    ],
    caller: 'billing.py',
    call: 'slack::post-message',
    result: 'sent',
  },
]

const BASELINE: (Entry & { worker: string })[] = [
  { id: 'http', kind: 'trigger', worker: 'http' },
  { id: 'state::get', kind: 'fn', worker: 'state' },
  { id: 'state::set', kind: 'fn', worker: 'state' },
]

const YAML_BASE = ['containers:', '  http:', '    version: "0.21.3"', '  state:', '    version: "0.22.2"']

const TICK_MS = 50
const CYCLE = 170
const FINAL = 10_000
const ease = easeOut

/** Timeline (in ticks) of one install, relative to the typed command's length. */
function timeline(cmdLength: number) {
  const typed = 8 + cmdLength
  const pull = typed + 6
  const started = pull + 12
  return {
    typedStart: 8,
    pull,
    started,
    entries: started + 4,
    yaml: started + 8,
    call: started + 30,
    result: started + 44,
  }
}

/**
 * Registry graphic: `compose::add` is typed in a terminal, the worker boots, its functions and
 * triggers appear in the live registry, the pinned version lands in `worker-compose.yaml`, and a
 * running agent (or any other worker) calls the new function. Loops through database, browser and slack.
 */
export function RegistryDemo({ className, embedded }: { className?: string; embedded?: boolean }) {
  const { ref, active } = useGraphicLoop<HTMLDivElement>()
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (!active) return
    const id = window.setInterval(() => setTick((t) => t + 1), TICK_MS)
    return () => window.clearInterval(id)
  }, [active])

  const index = active ? Math.floor(tick / CYCLE) % INSTALLS.length : 0
  const t = active ? tick % CYCLE : FINAL
  const w = INSTALLS[index]
  const command = `iii trigger compose::add worker=${w.worker}`
  const tl = timeline(command.length)
  const chars = Math.max(0, Math.min(command.length, t - tl.typedStart))
  const typing = t < tl.pull
  const fading = active && t > CYCLE - 8

  return (
    <div
      ref={ref}
      className={cn(
        embedded ? 'relative bg-card' : 'graphic-stage overflow-hidden rounded-2xl border bg-card',
        'flex flex-col overflow-hidden font-mono text-[12px] leading-relaxed',
        className,
      )}
      role="img"
      aria-label={`Running iii trigger compose::add worker=database starts the database worker; its functions and trigger appear in the live registry, version 0.8.3 is pinned in worker-compose.yaml, and an agent calls database::query on its next step.`}
    >
      <motion.div
        aria-hidden
        className="flex flex-1 flex-col"
        animate={{ opacity: fading ? 0.35 : 1 }}
        transition={{ duration: 0.35 }}
      >
        {/* Window chrome */}
        <div className="flex h-10 items-center gap-3 border-b px-4 text-muted-foreground text-xs">
          <span className="flex gap-1.5">
            <span className="size-2 rounded-full bg-line-strong/60" />
            <span className="size-2 rounded-full bg-line-strong/60" />
            <span className="size-2 rounded-full bg-line-strong/60" />
          </span>
          <span>
            <span className="text-foreground">iii</span> · terminal
          </span>
        </div>
        <div className="h-[92px] px-4 py-3 text-[13px]">
          <p className="flex min-w-0 whitespace-nowrap">
            <span className="mr-2 select-none text-muted-foreground">$</span>
            <span className="truncate text-foreground">{command.slice(0, chars)}</span>
            {typing ? (
              <motion.span
                className="ml-px inline-block h-[1.1em] w-[7px] translate-y-[3px] bg-foreground"
                animate={{ opacity: chars > 0 && chars < command.length ? 1 : [1, 1, 0, 0] }}
                transition={{ duration: 1, repeat: Number.POSITIVE_INFINITY, times: [0, 0.5, 0.5, 1] }}
              />
            ) : null}
          </p>
          <Line show={t >= tl.pull} className="text-muted-foreground">
            pulling {w.worker} {w.version}
          </Line>
          <Line show={t >= tl.started} className="flex items-center gap-2 text-muted-foreground">
            <span className="size-1.5 rounded-full bg-ok" />
            <span>
              <span className="text-foreground">{w.worker}</span> started ·{' '}
              {w.entries.filter((e) => e.kind === 'fn').length} functions,{' '}
              {w.entries.filter((e) => e.kind === 'trigger').length || 'no'} trigger
              {w.entries.filter((e) => e.kind === 'trigger').length === 1 ? '' : 's'}
            </span>
          </Line>
        </div>

        {/* Registry + compose file */}
        <div className="grid flex-1 border-t sm:grid-cols-[1.25fr_1fr] sm:divide-x">
          <div className="px-4 py-3">
            <p className="mb-2 flex items-center justify-between text-muted-foreground text-xs">
              <span>registry</span>
              <span className="flex items-center gap-1.5">
                <span className="size-1.5 rounded-full bg-ok" />
                live
              </span>
            </p>
            <ul className="h-[168px]">
              {BASELINE.map((e) => (
                <RegistryRow key={e.id} entry={e} worker={e.worker} />
              ))}
              <AnimatePresence initial={false}>
                {w.entries.map((e, i) =>
                  t >= tl.entries + i * 3 ? (
                    <RegistryRow
                      key={`${index}-${e.id}`}
                      entry={e}
                      worker={w.worker}
                      fresh
                      called={e.id === w.call && t >= tl.call}
                    />
                  ) : null,
                )}
              </AnimatePresence>
            </ul>
          </div>
          <div className="border-t px-4 py-3 sm:border-t-0">
            <p className="mb-2 text-muted-foreground text-xs">worker-compose.yaml</p>
            <pre className="text-[12px] text-muted-foreground leading-6">
              {YAML_BASE.map((l) => (
                <div key={l} className="pl-3">
                  {l}
                </div>
              ))}
              <AnimatePresence initial={false}>
                {t >= tl.yaml
                  ? [`  ${w.worker}:`, `    version: "${w.version}"`].map((l, i) => (
                      <motion.div
                        key={`${index}-${l}`}
                        className="-mx-4 relative bg-faint pr-4 pl-7 text-foreground"
                        initial={{ opacity: 0, x: -6 }}
                        animate={{ opacity: 1, x: 0 }}
                        exit={{ opacity: 0 }}
                        transition={{ duration: 0.35, delay: i * 0.08, ease }}
                      >
                        <span className="absolute left-4 text-ok">+</span>
                        {l}
                      </motion.div>
                    ))
                  : null}
              </AnimatePresence>
            </pre>
          </div>
        </div>

        {/* Next step of a running caller */}
        <div className="flex h-11 items-center gap-3 border-t px-4 text-xs">
          <AnimatePresence mode="wait" initial={false}>
            {t >= tl.call ? (
              <motion.div
                key={`call-${index}`}
                className="flex min-w-0 flex-1 items-center gap-2 whitespace-nowrap"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.3, ease }}
              >
                <span className="text-muted-foreground">{w.caller}</span>
                <span className="text-muted-foreground">→</span>
                <span className="truncate text-foreground">{w.call}</span>
                <motion.span
                  className="ml-auto flex items-center gap-1.5 text-muted-foreground"
                  initial={false}
                  animate={{ opacity: t >= tl.result ? 1 : 0 }}
                  transition={{ duration: 0.3 }}
                >
                  <span className="size-1.5 rounded-full bg-ok" />
                  {w.result}
                </motion.span>
              </motion.div>
            ) : (
              <motion.span
                key={`wait-${index}`}
                className="text-muted-foreground"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
              >
                agent · waiting for its next step
              </motion.span>
            )}
          </AnimatePresence>
        </div>
      </motion.div>
    </div>
  )
}

function Line({ show, className, children }: { show: boolean; className?: string; children: React.ReactNode }) {
  return (
    <motion.p
      className={cn('mt-1 whitespace-nowrap text-[12px]', className)}
      initial={false}
      animate={{ opacity: show ? 1 : 0, y: show ? 0 : 3 }}
      transition={{ duration: 0.3, ease }}
    >
      {children}
    </motion.p>
  )
}

function RegistryRow({
  entry,
  worker,
  fresh,
  called,
}: {
  entry: Entry
  worker: string
  fresh?: boolean
  called?: boolean
}) {
  return (
    <motion.li
      className={cn(
        '-mx-2 flex h-6 items-center gap-2 rounded-md px-2 transition-colors duration-300',
        called ? 'bg-faint' : fresh ? 'bg-muted/40' : '',
      )}
      initial={fresh ? { opacity: 0, x: -8 } : false}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.35, ease }}
    >
      <span
        className={cn(
          'w-14 shrink-0 rounded-[4px] border px-1 text-center text-[11px] leading-4',
          entry.kind === 'fn' ? 'bg-background text-muted-foreground' : 'border-dashed text-muted-foreground',
        )}
      >
        {entry.kind}
      </span>
      <span className={cn('truncate', called ? 'text-foreground' : fresh ? 'text-foreground' : 'text-foreground/80')}>
        {entry.id}
      </span>
      <span className="ml-auto shrink-0 text-[12px] text-muted-foreground">{worker}</span>
    </motion.li>
  )
}
