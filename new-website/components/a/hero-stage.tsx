'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { IconActivity, IconBot, IconCheckCircle, IconRocket, IconServer, IconTerminal } from '@/components/icons/iconly'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { demo } from './content'
import { OverviewGraph, SCRIPT, WORKERS } from './overview-graph'

/**
 * The hero's background film, built from our own components and recorded with Playwright at 1920×1080 (see
 * `app/a/stage`). It is cut the way a product recording would be: four big shots in sequence, each one filling
 * the frame so it still reads under the blur. Install in the terminal, the harness working through the graph,
 * the run as one trace, then the same graph deploying to every host. One 24 second loop; every clock runs off it,
 * so any 24 second cut of the recording loops without a seam.
 */

export const LOOP_S = 24

/** Shot boundaries in seconds. */
const SHOTS = [
  { id: 'install', from: 0, to: 5.6 },
  { id: 'harness', from: 5.6, to: 13.2 },
  { id: 'trace', from: 13.2, to: 18.4 },
  { id: 'deploy', from: 18.4, to: LOOP_S },
] as const
type ShotId = (typeof SHOTS)[number]['id']

type Line = { at: number; kind: 'cmd' | 'ok' | 'link'; text: string }

const TERMINAL: Line[] = [
  { at: 0.4, kind: 'cmd', text: 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh' },
  { at: 1.2, kind: 'ok', text: 'iii installed · press y to set up the harness' },
  { at: 1.8, kind: 'cmd', text: 'iii compose --up' },
  { at: 2.4, kind: 'ok', text: 'engine ready · ws://127.0.0.1:49134' },
  { at: 2.9, kind: 'ok', text: 'http ready (142ms) · database ready (233ms)' },
  { at: 3.5, kind: 'ok', text: 'harness ready (531ms) · llm-router · provider-anthropic' },
  { at: 4.1, kind: 'ok', text: 'browser ready (400ms) · github ready (286ms)' },
  { at: 4.8, kind: 'link', text: 'open http://127.0.0.1:3113' },
]

type Call = { at: number; fn: string; detail: string; ms: number; step: number }

/** The harness serving one prompt. `step` is the matching beat of the graph script. */
const PROMPT_AT = 6.2
const CALLS: Call[] = [
  { at: 7.2, fn: 'router::chat', detail: 'llm-router', ms: 920, step: 6 },
  { at: 8.4, fn: 'provider::anthropic::stream', detail: 'claude', ms: 884, step: 7 },
  { at: 9.6, fn: 'database::execute', detail: '4 rows', ms: 14, step: 8 },
  { at: 10.8, fn: 'github::pr::create', detail: '#418', ms: 33, step: 9 },
]
const REPLY_AT = 12.0
const TOTAL_MS = CALLS.reduce((sum, c) => sum + c.ms, 0)

/** Trace spans draw in the trace shot, one after another. */
const SPAN_AT = [13.7, 14.5, 15.3, 16.1]
const TRACE_DONE_AT = 17.0

type Host = { at: number; name: string; detail: string }

const DEPLOY_AT = 18.9
const HOSTS: Host[] = [
  { at: 19.7, name: 'Your laptop', detail: 'local' },
  { at: 20.5, name: 'iii Cloud', detail: 'managed' },
  { at: 21.3, name: 'AWS', detail: 'self-hosted' },
  { at: 22.1, name: 'GCP', detail: 'self-hosted' },
]
const DEPLOYED_AT = 22.7

/** Which graph beat plays at time t during the harness shot: everything joined, then the calls in order. */
function graphStep(t: number) {
  if (t < PROMPT_AT) return 4
  if (t < CALLS[0].at) return 5
  const live = CALLS.filter((c) => c.at <= t)
  if (t >= REPLY_AT) return 10
  return live[live.length - 1].step
}

/** Twenty ticks a second, for a smooth recording. */
function useStageClock() {
  const [clock, setClock] = useState({ t: 0, cycle: 0 })
  useEffect(() => {
    const started = performance.now()
    const id = window.setInterval(() => {
      const elapsed = (performance.now() - started) / 1000
      const cycle = Math.floor(elapsed / LOOP_S)
      setClock({ t: elapsed - cycle * LOOP_S, cycle })
    }, 50)
    return () => window.clearInterval(id)
  }, [])
  return clock
}

const ms = (v: number) => (v < 1000 ? `${v}ms` : `${(v / 1000).toFixed(2)}s`)

function Panel({
  icon: Icon,
  title,
  status,
  live,
  className,
  children,
}: {
  icon: React.ComponentType<{ className?: string }>
  title: string
  status?: string
  live?: boolean
  className?: string
  children: React.ReactNode
}) {
  return (
    <div className={cn('flex min-w-0 flex-col overflow-hidden rounded-3xl border bg-card', className)}>
      <div className="flex h-16 shrink-0 items-center gap-3 border-b bg-faint/60 px-6">
        <Icon className="size-5 shrink-0 text-muted-foreground" />
        <span className="font-sans text-[20px] text-muted-foreground">{title}</span>
        {status ? (
          <span className="ml-auto flex items-center gap-2.5 font-sans text-[18px] text-muted-foreground">
            <span className={cn('size-2.5 rounded-full', live ? 'bg-ok' : 'bg-line-strong')} />
            {status}
          </span>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 px-6 py-5">{children}</div>
    </div>
  )
}

function Enter({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <motion.li
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: easeOut }}
      className={className}
    >
      {children}
    </motion.li>
  )
}

function Shot({ name, children }: { name: string; children: React.ReactNode }) {
  return (
    <motion.div
      key={name}
      initial={{ opacity: 0, scale: 0.985 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 1.01 }}
      transition={{ duration: 0.45, ease: easeOut }}
      className="absolute inset-0 flex items-center justify-center"
    >
      {children}
    </motion.div>
  )
}

export function HeroStage() {
  const { t, cycle } = useStageClock()
  const shot: ShotId = (SHOTS.find((s) => t >= s.from && t < s.to) ?? SHOTS[0]).id
  const fading = t > LOOP_S - 0.45

  const terminal = TERMINAL.filter((l) => l.at <= t)
  const typing = t < TERMINAL[TERMINAL.length - 1].at
  const prompted = t >= PROMPT_AT
  const calls = CALLS.filter((c) => c.at <= t)
  const replied = t >= REPLY_AT
  const step = graphStep(t)
  const spansShown = SPAN_AT.filter((at) => at <= t).length
  const traced = t >= TRACE_DONE_AT
  const deploying = t >= DEPLOY_AT
  const hosts = HOSTS.filter((h) => h.at <= t)
  const deployed = t >= DEPLOYED_AT
  const joinedCount = WORKERS.length

  return (
    <div className="h-[1080px] w-[1920px] overflow-hidden bg-background text-foreground">
      <motion.div
        key={`cycle-${cycle}`}
        initial={{ opacity: 0 }}
        animate={{ opacity: fading ? 0 : 1 }}
        transition={{ duration: 0.4, ease: easeOut }}
        className="relative h-full w-full p-10"
      >
        {/* Console bar */}
        <div className="flex h-16 items-center gap-5 rounded-2xl border bg-card px-6">
          <span aria-hidden className="flex items-end gap-0.5">
            {[0, 1, 2].map((i) => (
              <span key={i} className="flex flex-col items-center gap-0.5">
                <span className="size-1.5 bg-foreground" />
                <span className="h-4 w-1.5 bg-foreground" />
              </span>
            ))}
          </span>
          <span className="font-sans text-[20px] text-foreground">iii console</span>
          <span className="font-mono text-[17px] text-muted-foreground">local · ws://127.0.0.1:49134</span>
          <span className="ml-auto flex items-center gap-3 font-sans text-[17px] text-muted-foreground">
            <span className="rounded-lg border px-3 py-1.5">
              {shot === 'install'
                ? `${terminal.filter((l) => l.kind === 'ok').length} ready`
                : `${joinedCount} workers`}
            </span>
            <span className="rounded-lg border px-3 py-1.5">
              {calls.length ? `${calls.length} spans` : 'no traces yet'}
            </span>
            <span className="rounded-lg border px-3 py-1.5">
              {deployed ? '4 hosts' : hosts.length ? `${hosts.length} hosts` : '1 host'}
            </span>
          </span>
        </div>

        <div className="relative mt-6 h-[920px]">
          <AnimatePresence mode="sync" initial={false}>
            {shot === 'install' ? (
              <Shot name="install">
                <Panel
                  icon={IconTerminal}
                  title="terminal"
                  status={typing ? 'installing' : 'engine up'}
                  live={t >= TERMINAL[3].at}
                  className="h-[760px] w-[1400px]"
                >
                  <ol className="flex flex-col gap-3 font-mono text-[26px] leading-[1.6]">
                    {terminal.map((line) => (
                      <Enter key={line.text} className="flex min-w-0 items-start gap-4">
                        {line.kind === 'cmd' ? (
                          <span className="shrink-0 text-hero-accent">$</span>
                        ) : line.kind === 'ok' ? (
                          <IconCheckCircle className="mt-[9px] size-6 shrink-0 text-ok" />
                        ) : (
                          <span className="shrink-0 text-muted-foreground">→</span>
                        )}
                        <span
                          className={cn(
                            'min-w-0 [overflow-wrap:anywhere]',
                            line.kind === 'cmd'
                              ? 'text-foreground'
                              : line.kind === 'link'
                                ? 'text-hero-accent'
                                : 'text-muted-foreground',
                          )}
                        >
                          {line.text}
                        </span>
                      </Enter>
                    ))}
                    {typing ? (
                      <li aria-hidden className="flex items-center gap-4">
                        <span className="text-hero-accent">$</span>
                        <motion.span
                          className="inline-block h-[28px] w-[14px] bg-foreground"
                          animate={{ opacity: [1, 0, 1] }}
                          transition={{ duration: 1, repeat: Number.POSITIVE_INFINITY, ease: 'linear' }}
                        />
                      </li>
                    ) : null}
                  </ol>
                </Panel>
              </Shot>
            ) : null}

            {shot === 'harness' ? (
              <Shot name="harness">
                <div className="grid h-[880px] w-full grid-cols-[640px_minmax(0,1fr)] gap-6">
                  <Panel
                    icon={IconBot}
                    title="harness · session 7f3a"
                    status={replied ? 'done' : prompted ? 'working' : 'idle'}
                    live={prompted && !replied}
                  >
                    <ol className="flex flex-col gap-3">
                      {prompted ? (
                        <Enter className="rounded-xl bg-faint px-4 py-3 font-sans text-[21px] text-foreground leading-snug">
                          › {demo.prompt}
                        </Enter>
                      ) : null}
                      {calls.map((call) => (
                        <Enter key={call.fn} className="flex min-w-0 items-center gap-3 px-1 font-sans text-[20px]">
                          <IconCheckCircle className="size-5 shrink-0 text-hero-accent" />
                          <span className="truncate font-mono text-foreground">{call.fn}</span>
                          <span className="truncate text-muted-foreground">{call.detail}</span>
                          <span className="ml-auto shrink-0 font-mono text-[17px] text-muted-foreground tabular-nums">
                            {ms(call.ms)}
                          </span>
                        </Enter>
                      ))}
                      {replied ? (
                        <Enter className="px-1 pt-2 font-sans text-[20px] text-foreground">
                          Digest posted to #releases · 3 PRs reviewed · watching #418
                        </Enter>
                      ) : null}
                    </ol>
                  </Panel>
                  <Panel
                    icon={IconActivity}
                    title="graph"
                    status={`${joinedCount} workers · ${SCRIPT[step].kind === 'call' ? 'routing' : 'live'}`}
                    live={SCRIPT[step].kind === 'call'}
                  >
                    <div className="flex h-full flex-col justify-center">
                      <OverviewGraph
                        step={step}
                        cycle={cycle}
                        active
                        label="The iii graph: a request on the left, the engine in the middle, seven workers on the right."
                      />
                      <p className="mt-6 text-center font-sans text-[19px] text-muted-foreground">
                        {SCRIPT[step].caption}
                      </p>
                    </div>
                  </Panel>
                </div>
              </Shot>
            ) : null}

            {shot === 'trace' ? (
              <Shot name="trace">
                <Panel
                  icon={IconActivity}
                  title="trace 7f3a2c9e · agent::run"
                  status={traced ? `${CALLS.length} spans · ${ms(TOTAL_MS)}` : 'recording'}
                  live={!traced}
                  className="h-[560px] w-[1400px]"
                >
                  <ol className="flex flex-col gap-4 pt-2">
                    {CALLS.map((call, i) => {
                      const shown = i < spansShown
                      const start = CALLS.slice(0, i).reduce((sum, c) => sum + c.ms, 0)
                      return (
                        <li
                          key={call.fn}
                          className={cn(
                            'grid h-14 grid-cols-[420px_minmax(0,1fr)_90px] items-center gap-6 transition-opacity duration-300',
                            shown ? 'opacity-100' : 'opacity-25',
                          )}
                        >
                          <span className="flex min-w-0 items-center gap-3 font-mono text-[22px]">
                            <span
                              className={cn(
                                'size-3 shrink-0 rounded-[3px]',
                                shown ? 'bg-ok' : 'border border-line-strong',
                              )}
                            />
                            <span className="truncate text-foreground">{call.fn}</span>
                          </span>
                          <span className="relative h-6">
                            <span aria-hidden className="absolute inset-y-0 left-0 w-px bg-line-strong" />
                            <motion.span
                              className={cn(
                                'absolute inset-y-0 rounded-[5px]',
                                i === 0 ? 'bg-hero-accent' : 'bg-foreground/70',
                              )}
                              style={{
                                left: `${(start / TOTAL_MS) * 100}%`,
                                width: `${(call.ms / TOTAL_MS) * 100}%`,
                                minWidth: 8,
                                transformOrigin: 'left center',
                              }}
                              initial={false}
                              animate={{ scaleX: shown ? 1 : 0 }}
                              transition={{ duration: 0.5, ease: easeOut }}
                            />
                          </span>
                          <motion.span
                            initial={false}
                            animate={{ opacity: shown ? 1 : 0 }}
                            transition={{ duration: 0.3, ease: easeOut }}
                            className="text-right font-mono text-[18px] text-muted-foreground tabular-nums"
                          >
                            {ms(call.ms)}
                          </motion.span>
                        </li>
                      )
                    })}
                  </ol>
                  <motion.p
                    initial={false}
                    animate={{ opacity: traced ? 1 : 0 }}
                    transition={{ duration: 0.3, ease: easeOut }}
                    className="mt-8 font-sans text-[20px] text-muted-foreground"
                  >
                    one trace across 4 workers · TypeScript, Python and Rust · exported over OTLP
                  </motion.p>
                </Panel>
              </Shot>
            ) : null}

            {shot === 'deploy' ? (
              <Shot name="deploy">
                <Panel
                  icon={IconRocket}
                  title="deploy"
                  status={deployed ? 'running everywhere' : deploying ? 'deploying' : 'local'}
                  live={deploying && !deployed}
                  className="h-[560px] w-[1400px]"
                >
                  <ol className="flex flex-col gap-4 pt-2">
                    {deploying ? (
                      <Enter className="flex items-center gap-4 font-mono text-[26px]">
                        <span className="text-hero-accent">$</span>
                        <span className="text-foreground">iii cloud deploy</span>
                      </Enter>
                    ) : null}
                    {hosts.map((host) => (
                      <Enter key={host.name} className="flex items-center gap-4 px-1 font-sans text-[24px]">
                        <IconServer className="size-6 shrink-0 text-muted-foreground" />
                        <span className="text-foreground">{host.name}</span>
                        <span className="text-muted-foreground">{host.detail}</span>
                        <IconCheckCircle className="ml-auto size-6 shrink-0 text-ok" />
                      </Enter>
                    ))}
                    {deployed ? (
                      <Enter className="px-1 pt-3 font-sans text-[22px] text-muted-foreground">
                        same graph, same workers, any host
                      </Enter>
                    ) : null}
                  </ol>
                </Panel>
              </Shot>
            ) : null}
          </AnimatePresence>
        </div>
      </motion.div>
    </div>
  )
}
