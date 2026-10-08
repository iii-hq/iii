'use client'

import { AnimatePresence, motion } from 'motion/react'

import { cn } from '@/lib/utils'

import { Dot, EngineHub, Flow, Node, STROKE, T, Tag, TYPE, Wire } from './kit'
import {
  atLeast,
  busPath,
  type Col,
  ENGINE,
  ENVIRONMENTS,
  GRID,
  HOPS,
  LATER,
  otlpRoute,
  routeFromEngine,
  routeToEngine,
  type Stage,
  stubPath,
  trunkPath,
  VIEW,
  WORKERS,
  workerById,
} from './model'

type Props = {
  stage: Stage
  step: number
  active: boolean
  /** Increments each time the stage's script loops; keys one-shot animations. */
  cycle?: number
  selected?: string | null
  onSelect?: (id: string) => void
  className?: string
  /** Accessible description of what the graph shows at this stage. */
  label: string
}

const ORIGIN: Record<string, string> = { registry: 'registry', written: 'agent-written', human: 'you added' }

/** Compose files: a quiet filled area behind each cluster, named by its tag. */
const GROUPS = {
  local: { x: 12, y: 68, w: 260, h: 332, file: 'worker-compose-local.yaml' },
  gpu: { x: 768, y: 68, w: 260, h: 188, file: 'worker-compose-gpu.yaml' },
} as const

/** Composability's cross-worker chain: browser → extract → embed → postgres, each hop through the engine. */
const CHAIN = ['browser', 'extract', 'embed', 'pg']
/** What the harness finds already running when it reads the demo prompt. */
const FOUND = ['github', 'browser', 'llm']

const TOTAL_MS = HOPS.reduce((sum, h) => sum + h.ms, 0)
const short = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`)
/** Span length as a share of the node width, on a log scale so 9 ms is still visible next to 1,840 ms. */
const spanShare = (ms: number) => Math.min(1, (Math.log10(ms) + 0.5) / 4)

const requestRoute = [
  { x: LATER.request.x + LATER.request.w / 2, y: GRID.topRow },
  { x: GRID.left.bus, y: GRID.topRow },
  { x: GRID.left.bus, y: ENGINE.y },
  { x: ENGINE.x - ENGINE.w / 2, y: ENGINE.y },
]
const registryRoute = [
  { x: LATER.registry.x - LATER.registry.w / 2, y: GRID.topRow },
  { x: GRID.right.bus, y: GRID.topRow },
  { x: GRID.right.bus, y: ENGINE.y },
  { x: ENGINE.x + ENGINE.w / 2, y: ENGINE.y },
]
const reverse = <P,>(pts: P[]) => [...pts].reverse()

const toEngine = (id: string) => routeToEngine(workerById[id])
const fromEngine = (id: string) => routeFromEngine(workerById[id])

const SUMMARY_LABELS = [
  ['C', 'Composability', GROUPS.local.x + 16, GROUPS.local.y + 20, 'start'],
  ['O', 'Observability', ENGINE.x, 228, 'middle'],
  ['D', 'Discoverability', GRID.right.x - 12, GRID.topRow, 'end'],
  ['E', 'Extensibility', LATER.camera.x, LATER.camera.y + LATER.camera.h / 2 + 24, 'middle'],
  ['R', 'Reactivity', LATER.digest.x, LATER.digest.y + LATER.digest.h / 2 + 24, 'middle'],
] as const

const envX = (i: number) => ENGINE.x + (i - 1.5) * 170

/** What is happening at this beat, in one line. Shared by the SVG's owners for narration. */
export function captionFor(stage: Stage, step: number): string | undefined {
  const observing = stage === 'observe' && step < HOPS.length
  return stage === 'foundation'
    ? step === 7
      ? 'Local, Cloud, Browser and Edge merge into one engine'
      : step >= 8
        ? 'one execution graph'
        : undefined
    : stage === 'compose'
      ? step === 4
        ? 'functions compose across workers'
        : step >= 2
          ? 'one engine · two compose files'
          : undefined
      : stage === 'observe'
        ? observing
          ? `span ${step + 1} of ${HOPS.length} · ${HOPS[step].fn}`
          : `1 trace · ${HOPS.length} spans · ${TOTAL_MS.toLocaleString('en-US')} ms`
        : stage === 'discover'
          ? [
              'the prompt needs 5 capabilities',
              'internal registry · 3 of 5 already here',
              'searching workers.iii.dev for slack',
              'workers.iii.dev · slack@1.2.0 found',
              'installed · slack::post joins',
              'nothing formats a digest · write it',
              'four found or installed · in use now',
            ][step]
          : stage === 'extend'
            ? [
                'digest::format written by the harness',
                'a camera · just a device with code',
                'through the Worker SDK',
                'camera::capture, stream, move exposed',
                'joined · callable from anywhere',
                'agent::run → camera::capture',
              ][step]
            : stage === 'react'
              ? [
                  'github::pr::watch emits pr.opened',
                  'two observers react',
                  'agent::review attaches · upstream untouched',
                  'pr.opened again',
                  'three observers react · no new call sites',
                  "observation isn't telemetry · it's composition",
                ][step]
              : stage === 'summary' && step >= 5
                ? 'C · O · D · E · R'
                : undefined
}

/**
 * The one iii graph. Stages are cumulative: everything that joined in an earlier stage stays, and the current
 * stage's script (`step`) animates what is new. Sections own the clock and the captions.
 */
export function SystemGraph({ stage, step, active, cycle = 0, selected, onSelect, className, label }: Props) {
  const at = (s: Stage) => atLeast(stage, s)
  const past = (s: Stage) => stage !== s && at(s)
  const executing = (stage === 'execution' || stage === 'harness') && step < HOPS.length
  const hop = executing ? HOPS[step] : null
  const prevHop = executing && step > 0 ? HOPS[step - 1] : null
  const done = (stage === 'execution' || stage === 'harness') && step === HOPS.length
  const observing = stage === 'observe' && step < HOPS.length
  const key = `${stage}-${cycle}-${step}`
  const runners = active
  const fadeIdle = stage === 'harness'

  const workerVisible = (i: number) => stage !== 'foundation' || !active || step > i
  const litWorker = (id: string) => {
    if (hop?.node === id) return true
    if (observing && HOPS[step].node === id) return true
    if (stage === 'compose' && step === 3 && (id === 'llm' || id === 'pg')) return true
    if (stage === 'compose' && step === 4 && CHAIN.includes(id)) return true
    if (stage === 'extend' && step === 5 && id === 'agent') return true
    if (stage === 'discover' && step === 0 && id === 'agent') return true
    if (stage === 'discover' && step === 1 && FOUND.includes(id)) return true
    if (stage === 'discover' && step >= 5 && id === 'agent') return true
    if (stage === 'react' && (step === 0 || step === 3) && id === 'github') return true
    return false
  }

  const spanVisible = (i: number) => past('observe') || (stage === 'observe' && step >= i)
  const requestVisible = at('execution')
  const registryVisible = past('discover') || (stage === 'discover' && step >= 2)
  const slackVisible = past('discover') || (stage === 'discover' && step >= 4)
  const digestVisible = past('extend') || (stage === 'extend' && step >= 0)
  const otlpVisible = past('observe') || (stage === 'observe' && step >= 9)
  const reviewVisible = past('react') || (stage === 'react' && step >= 2)
  const cameraVisible = past('extend') || (stage === 'extend' && step >= 3)
  const cameraJoined = past('extend') || (stage === 'extend' && step >= 4)
  const observersLit = stage === 'react' && (step === 1 || step === 4)
  const merged = stage !== 'foundation' || !active || step >= 7
  const envLine = stage !== 'foundation' || !active || step >= 8

  /* Bus extents: from the top-most joined stub down to the lowest, always reaching the engine's mid-line. */
  const columnYs = (col: Col) => {
    const ys: number[] = [ENGINE.y]
    WORKERS.forEach((w, i) => {
      if (w.col === col && workerVisible(i)) ys.push(w.y)
    })
    if (col === 'left') {
      if (requestVisible) ys.push(GRID.topRow)
      if (slackVisible) ys.push(LATER.slack.y)
      if (digestVisible) ys.push(LATER.digest.y)
    } else {
      if (registryVisible) ys.push(GRID.topRow)
      if (reviewVisible) ys.push(LATER.review.y)
      if (cameraVisible) ys.push(LATER.camera.y)
    }
    return { top: Math.min(...ys), bottom: Math.max(...ys) }
  }
  const left = columnYs('left')
  const right = columnYs('right')

  const pulse =
    active &&
    (executing ||
      (stage === 'foundation' && step === 7) ||
      observersLit ||
      (stage === 'discover' && step === 1) ||
      (stage === 'compose' && step === 4))

  return (
    <svg
      viewBox={`0 0 ${VIEW.w} ${VIEW.h}`}
      role="img"
      aria-label={label}
      className={cn('h-auto w-full overflow-visible', className)}
    >
      {/* Compose files */}
      {at('compose')
        ? (Object.keys(GROUPS) as (keyof typeof GROUPS)[]).map((id, i) => {
            const g = GROUPS[id]
            const visible = past('compose') || step >= i
            return (
              <motion.g
                key={id}
                initial={false}
                animate={{ opacity: visible ? 1 : 0 }}
                transition={{ duration: 0.6 * T }}
              >
                <rect x={g.x} y={g.y} width={g.w} height={g.h} rx={14} fill="var(--faint)" />
                {stage !== 'summary' ? (
                  <text
                    x={g.x + 28}
                    y={g.y + 27}
                    className={stage === 'compose' ? 'fill-foreground font-mono' : 'fill-muted-foreground font-mono'}
                    fontSize={TYPE.label}
                  >
                    {g.file}
                  </text>
                ) : null}
              </motion.g>
            )
          })
        : null}

      {/* Buses and trunks */}
      <motion.path
        d={busPath('left', left.top, left.bottom)}
        fill="none"
        stroke="var(--line-strong)"
        strokeWidth={STROKE}
        strokeLinecap="round"
        initial={false}
        animate={{ d: busPath('left', left.top, left.bottom) }}
        transition={{ duration: 0.5 * T }}
      />
      <motion.path
        d={busPath('right', right.top, right.bottom)}
        fill="none"
        stroke="var(--line-strong)"
        strokeWidth={STROKE}
        strokeLinecap="round"
        initial={false}
        animate={{
          d: busPath('right', right.top, right.bottom),
          opacity: right.top === ENGINE.y && right.bottom === ENGINE.y ? 0 : 1,
        }}
        transition={{ duration: 0.5 * T }}
      />
      <Wire d={trunkPath('left')} />
      <Wire d={trunkPath('right')} visible={right.top !== ENGINE.y || right.bottom !== ENGINE.y} />

      {/* Stubs */}
      {WORKERS.map((w, i) => (
        <Wire key={w.id} d={stubPath(w)} visible={workerVisible(i)} delay={0.1} />
      ))}
      <Wire
        d={`M ${LATER.request.x + LATER.request.w / 2} ${GRID.topRow} H ${GRID.left.bus - 8}`}
        visible={requestVisible}
      />
      <Wire
        d={`M ${LATER.registry.x - LATER.registry.w / 2} ${GRID.topRow} H ${GRID.right.bus + 8}`}
        visible={registryVisible}
        dashed
        tone="ghost"
      />
      <Wire d={stubPath(LATER.slack)} visible={slackVisible} />
      <Wire d={stubPath(LATER.digest)} visible={digestVisible} />
      <Wire d={stubPath(LATER.review)} visible={reviewVisible} dashed />
      <Wire d={stubPath(LATER.camera)} visible={cameraJoined} />
      <Wire d={`M ${otlpRoute[0].x} ${otlpRoute[0].y} V ${otlpRoute[1].y}`} visible={otlpVisible} dashed tone="ghost" />

      {/* Engine */}
      <EngineHub
        pulseKey={pulse ? key : undefined}
        lit={Boolean(hop) || observersLit}
        environments={envLine ? ENVIRONMENTS.map((e) => e.toLowerCase()).join(' · ') : undefined}
      />

      {/* Overview: the environments sit under the engine, then rise into it and become its second line */}
      {stage === 'foundation' && active
        ? ENVIRONMENTS.map((env, i) => (
            <motion.g
              key={`${env}-${cycle}`}
              initial={{ opacity: 0, y: 8 }}
              animate={{
                x: merged ? ENGINE.x - envX(i) : 0,
                y: merged ? ENGINE.y + ENGINE.h / 2 + 10 - LATER.otlp.y : 0,
                opacity: merged ? [1, 1, 0] : 1,
                scale: merged ? [1, 1, 0.8] : 1,
              }}
              transition={
                merged
                  ? {
                      duration: 0.9 * T,
                      delay: i * 0.08,
                      ease: [0.65, 0, 0.35, 1],
                      opacity: { duration: 0.9 * T, delay: i * 0.08, times: [0, 0.55, 0.9] },
                      scale: { duration: 0.9 * T, delay: i * 0.08, times: [0, 0.55, 1] },
                    }
                  : { duration: 0.5 * T, delay: 0.3 + i * 0.08, ease: [0.22, 1, 0.36, 1] }
              }
              style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
            >
              <Tag x={envX(i)} y={LATER.otlp.y} anchor="middle" tone="foreground">
                {env}
              </Tag>
            </motion.g>
          ))
        : null}

      {/* Workers */}
      {WORKERS.map((w, i) => {
        const visible = workerVisible(i)
        const joined = at('execution')
        const tone = selected === w.id ? 'selected' : litWorker(w.id) ? 'lit' : 'idle'
        const spans = HOPS.map((h, hi) => ({ ...h, hi })).filter((h) => h.node === w.id)
        const spanShown = at('observe') && spans.some((h) => spanVisible(h.hi))
        const spanMs = spans.filter((h) => spanVisible(h.hi)).reduce((sum, h) => sum + h.ms, 0)
        const spanLit = observing && spans.some((h) => h.hi === step)
        const event = stage === 'react' && w.id === 'github' && (step === 0 || step === 3)
        const meta =
          stage === 'foundation' && active ? ORIGIN[w.origin] : event ? 'pr.opened' : spanShown ? short(spanMs) : w.kind
        const metaTone = (stage === 'foundation' && active && step === i) || event || spanLit ? 'accent' : 'muted'
        return (
          <g key={w.id}>
            <Node
              cx={w.x}
              cy={w.y}
              w={w.w}
              h={w.h}
              title={joined ? w.worker : w.role}
              sub={joined ? w.fn : undefined}
              meta={meta}
              metaTone={metaTone}
              bar={spanShown ? spanShare(spanMs) : undefined}
              barLit={spanLit}
              tone={tone}
              visible={visible}
              faded={fadeIdle && tone === 'idle'}
              onClick={onSelect ? () => onSelect(w.id) : undefined}
              label={`${w.worker}: ${w.fn}`}
            />
            {done && w.id === 'github' ? <Dot cx={w.x + w.w / 2} cy={w.y - w.h / 2} /> : null}
            {stage === 'discover' && step >= 1 && step <= 4 && FOUND.includes(w.id) ? (
              <Dot
                cx={w.x + (w.col === 'left' ? w.w / 2 : -w.w / 2)}
                cy={w.y - w.h / 2}
                delay={FOUND.indexOf(w.id) * 0.12}
              />
            ) : null}
          </g>
        )
      })}

      {/* Request and registry, top row */}
      <Node
        cx={LATER.request.x}
        cy={LATER.request.y}
        w={LATER.request.w}
        h={LATER.request.h}
        title="› request"
        center
        visible={requestVisible}
        tone={executing && step === 0 ? 'lit' : 'idle'}
        faded={fadeIdle && !(executing && step === 0)}
      />
      <Node
        cx={LATER.registry.x}
        cy={LATER.registry.y}
        w={LATER.registry.w}
        h={LATER.registry.h}
        title="workers.iii.dev"
        center
        dashed
        visible={registryVisible}
        tone={stage === 'discover' && (step === 2 || step === 3) ? 'lit' : 'idle'}
        faded={fadeIdle}
      />

      {/* Later workers */}
      <Node
        cx={LATER.slack.x}
        cy={LATER.slack.y}
        w={LATER.slack.w}
        h={LATER.slack.h}
        title="slack"
        sub="slack::post"
        meta={stage === 'discover' ? 'installed' : 'ts'}
        metaTone={stage === 'discover' && step === 4 ? 'accent' : 'muted'}
        visible={slackVisible}
        tone={(stage === 'discover' && step === 6) || observersLit ? 'lit' : 'idle'}
        faded={fadeIdle}
      />
      <Node
        cx={LATER.digest.x}
        cy={LATER.digest.y}
        w={LATER.digest.w}
        h={LATER.digest.h}
        title="digest"
        sub="digest::format"
        meta={stage === 'extend' ? 'harness-written' : 'ai'}
        metaTone={stage === 'extend' && step <= 1 ? 'accent' : 'muted'}
        visible={digestVisible}
        tone={observersLit ? 'lit' : 'idle'}
        faded={fadeIdle}
      />
      <Node
        cx={LATER.review.x}
        cy={LATER.review.y}
        w={LATER.review.w}
        h={LATER.review.h}
        title="agent"
        sub="agent::review"
        meta={stage === 'react' ? 'observer' : 'ai'}
        metaTone={stage === 'react' && step === 2 ? 'accent' : 'muted'}
        visible={reviewVisible}
        tone={stage === 'react' && step === 4 ? 'lit' : 'idle'}
        faded={fadeIdle}
      />
      <Node
        cx={LATER.camera.x}
        cy={LATER.camera.y}
        w={LATER.camera.w}
        h={LATER.camera.h}
        title="camera"
        sub={['camera::capture', 'camera::stream', 'camera::move']}
        meta={stage === 'extend' ? 'worker sdk' : 'rs'}
        metaTone={stage === 'extend' && step === 3 ? 'accent' : 'muted'}
        visible={cameraVisible}
        tone={stage === 'extend' && step === 5 ? 'lit' : 'idle'}
        faded={fadeIdle}
      />
      {stage === 'extend' && cameraJoined ? (
        <Dot cx={LATER.camera.x - LATER.camera.w / 2} cy={LATER.camera.y - LATER.camera.h / 2} delay={0.4} />
      ) : null}
      {stage === 'extend' && step === 0 ? (
        <Dot cx={LATER.digest.x + LATER.digest.w / 2} cy={LATER.digest.y - LATER.digest.h / 2} delay={0.4} />
      ) : null}

      {/* Observability: the trace leaves the system over OTLP */}
      <Node
        cx={LATER.otlp.x}
        cy={LATER.otlp.y}
        w={LATER.otlp.w}
        h={LATER.otlp.h}
        title="otlp → your stack"
        center
        dashed
        visible={otlpVisible}
        tone={stage === 'observe' && step === 9 ? 'lit' : 'idle'}
        faded={fadeIdle}
      />

      {/* Summary labels */}
      {stage === 'summary'
        ? SUMMARY_LABELS.map(([letter, word, x, y, anchor], i) => (
            <Tag key={letter} x={x} y={y} anchor={anchor} tone="foreground" visible={step >= i} delay={0.05}>
              {`${letter}  ${word}`}
            </Tag>
          ))
        : null}

      {/* Packets */}
      <AnimatePresence>
        {runners && hop ? (
          <motion.g key={key} exit={{ opacity: 0 }} transition={{ duration: 0.2 }}>
            <Flow points={prevHop ? toEngine(prevHop.node) : requestRoute} duration={0.4} />
            <Flow points={fromEngine(hop.node)} delay={0.42} duration={0.4} />
          </motion.g>
        ) : null}
        {runners && stage === 'compose' && step === 3 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={toEngine('llm')} duration={0.45} />
            <Flow points={fromEngine('pg')} delay={0.47} duration={0.45} />
          </motion.g>
        ) : null}
        {runners && stage === 'compose' && step === 4 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            {CHAIN.slice(0, -1).map((id, i) => (
              <g key={id}>
                <Flow points={toEngine(id)} delay={i * 0.5} duration={0.24} />
                <Flow points={fromEngine(CHAIN[i + 1])} delay={i * 0.5 + 0.25} duration={0.24} />
              </g>
            ))}
          </motion.g>
        ) : null}
        {runners && stage === 'observe' && step === 9 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={otlpRoute} duration={0.6} />
          </motion.g>
        ) : null}
        {runners && stage === 'extend' && step === 5 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={toEngine('agent')} duration={0.45} />
            <Flow points={routeFromEngine(LATER.camera)} delay={0.47} duration={0.45} />
          </motion.g>
        ) : null}
        {runners && stage === 'discover' && step === 1 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={toEngine('agent')} duration={0.4} />
            {FOUND.map((id) => (
              <Flow key={id} points={fromEngine(id)} delay={0.42} duration={0.4} />
            ))}
          </motion.g>
        ) : null}
        {runners && stage === 'discover' && step === 2 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={reverse(registryRoute)} duration={0.5} />
          </motion.g>
        ) : null}
        {runners && stage === 'discover' && step === 3 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={registryRoute} duration={0.5} />
          </motion.g>
        ) : null}
        {runners && stage === 'discover' && step === 6 ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={toEngine('agent')} duration={0.4} />
            <Flow points={routeFromEngine(LATER.slack)} delay={0.42} duration={0.4} />
          </motion.g>
        ) : null}
        {runners && observersLit ? (
          <motion.g key={key} exit={{ opacity: 0 }}>
            <Flow points={toEngine('github')} duration={0.4} tone="event" />
            <Flow points={routeFromEngine(LATER.slack)} delay={0.42} duration={0.4} tone="event" />
            <Flow points={routeFromEngine(LATER.digest)} delay={0.42} duration={0.4} tone="event" />
            {step === 4 ? (
              <Flow points={routeFromEngine(LATER.review)} delay={0.42} duration={0.4} tone="event" />
            ) : null}
          </motion.g>
        ) : null}
      </AnimatePresence>
    </svg>
  )
}
