'use client'

import { motion } from 'motion/react'

import { easeOut } from '@/lib/motion'

import { CoderFrame, KindChip, SvgLabel, useCoderSteps } from './coder-kit'

type Span = { id: string; kind: string; depth: number; start: number; end: number }
type Log = { span: string; at: number; text: string }

const SPANS: Span[] = [
  { id: 'agent::turn', kind: 'ai', depth: 0, start: 0, end: 1.3 },
  { id: 'llm::complete', kind: 'ai', depth: 1, start: 0.03, end: 0.62 },
  { id: 'tickets::get', kind: 'ts', depth: 1, start: 0.66, end: 0.8 },
  { id: 'repo::search', kind: 'rs', depth: 1, start: 0.83, end: 1.05 },
  { id: 'tests::run', kind: 'go', depth: 1, start: 1.07, end: 1.25 },
  { id: 'queue::enqueue', kind: 'ts', depth: 1, start: 1.26, end: 1.29 },
  { id: 'report::build', kind: 'py', depth: 2, start: 1.42, end: 2.14 },
]

const LOGS: Log[] = [
  { span: 'tests::run', at: 1.2, text: 'INFO 42 passed' },
  { span: 'report::build', at: 2.02, text: 'INFO wrote summary.md' },
]

type Row = { type: 'span'; span: Span } | { type: 'log'; log: Log; depth: number }

const ROWS: Row[] = SPANS.flatMap((span) => {
  const rows: Row[] = [{ type: 'span', span }]
  for (const log of LOGS) if (log.span === span.id) rows.push({ type: 'log', log, depth: span.depth + 1 })
  return rows
})

const X0 = 180
const X1 = 444
const T = 2.2
const tx = (t: number) => X0 + (t / T) * (X1 - X0)
const rowY = (i: number) => 76 + i * 23

/** Seconds of animation per second of trace time. */
const K = 2.5
const EXPORT_AT = T * K + 0.3
const TARGETS = ['Grafana', 'Jaeger', 'Datadog', 'ADE']

const CAPTIONS = [
  'agent::turn → llm::complete',
  'tickets::get · repo::search · tests::run',
  'queue::enqueue → report::build in Python, same trace',
  'logs attach to the span that wrote them',
  'OTLP → Grafana · Jaeger · Datadog · ADE',
  'one agent turn · 7 spans · 4 languages · 1 waterfall',
]

/**
 * Observability: the OpenTelemetry waterfall of one agent turn. A model call, three function
 * spans and an enqueued Python job share one trace, logs attach to their spans, then it exports.
 */
export function CoderObserve() {
  const { ref, active, step, cycle } = useCoderSteps(CAPTIONS.length, 1500)
  const live = active

  const enqueueRow = ROWS.findIndex((r) => r.type === 'span' && r.span.id === 'queue::enqueue')
  const buildRow = ROWS.findIndex((r) => r.type === 'span' && r.span.id === 'report::build')

  return (
    <CoderFrame
      frameRef={ref}
      label="A trace waterfall for one agent turn: a model call, three function spans in TypeScript, Rust and Go, and an enqueued Python job that continues the same trace. Log lines attach to their spans, and the trace exports over OTLP to Grafana, Jaeger, Datadog or the ADE."
      caption={CAPTIONS[step]}
      captionKey={step}
    >
      {/* Header */}
      <SvgLabel x={16} y={24}>
        TRACE
      </SvgLabel>
      <text x={62} y={24} className="fill-foreground font-mono" fontSize={10.5}>
        4bf92f3c
      </text>
      <text x={444} y={24} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={9.5}>
        7 spans · 2 logs · 2.14s
      </text>
      <line x1={16} x2={444} y1={36} y2={36} stroke="var(--line)" />

      {/* Time axis */}
      {[0, 0.5, 1, 1.5, 2].map((t) => (
        <g key={t}>
          <line
            x1={tx(t)}
            x2={tx(t)}
            y1={56}
            y2={rowY(ROWS.length - 1) + 10}
            stroke="var(--line)"
            strokeDasharray="2 4"
          />
          <text
            x={tx(t)}
            y={52}
            textAnchor={t === 0 ? 'start' : 'middle'}
            className="fill-muted-foreground font-mono"
            fontSize={8.5}
          >
            {t === 0 ? '0' : t < 1 ? `${t * 1000}ms` : `${t.toFixed(1)}s`}
          </text>
        </g>
      ))}

      <g key={live ? cycle : 'static'}>
        {/* Queue hop: enqueue → Python job continues the trace */}
        <motion.path
          d={`M ${tx(1.285)} ${rowY(enqueueRow) + 5} V ${rowY(buildRow)} H ${tx(1.42)}`}
          fill="none"
          stroke="var(--line-strong)"
          strokeDasharray="3 3"
          initial={live ? { pathLength: 0 } : false}
          animate={{ pathLength: 1 }}
          transition={{ duration: 0.35 * K, delay: 1.29 * K, ease: 'linear' }}
        />

        {ROWS.map((row, i) => {
          const y = rowY(i)
          if (row.type === 'log') {
            const lx = tx(row.log.at)
            const spanRow = i - 1
            return (
              <motion.g
                key={row.log.text}
                initial={live ? { opacity: 0, y: -4 } : false}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.4, delay: row.log.at * K, ease: easeOut }}
              >
                <text
                  x={16 + row.depth * 12 + 22}
                  y={y + 3.5}
                  className="fill-muted-foreground font-mono"
                  fontSize={9.5}
                >
                  ↳ log
                </text>
                <line x1={lx} x2={lx} y1={rowY(spanRow) + 5} y2={y - 1} stroke="var(--line-strong)" />
                <circle cx={lx} cy={rowY(spanRow)} r={2.5} fill="var(--background)" stroke="var(--hero-accent)" />
                <text
                  x={row.log.at > 1.8 ? lx - 6 : lx + 6}
                  y={y + 3.5}
                  textAnchor={row.log.at > 1.8 ? 'end' : 'start'}
                  className="fill-foreground font-mono"
                  fontSize={9.5}
                >
                  {row.log.text}
                </text>
              </motion.g>
            )
          }
          const { span } = row
          const x = 16 + span.depth * 12
          const w = Math.max(tx(span.end) - tx(span.start), 3)
          return (
            <g key={span.id}>
              <motion.g
                initial={live ? { opacity: 0.35 } : false}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.3, delay: span.start * K }}
              >
                <KindChip x={x} y={y - 8} kind={span.kind} size={16} />
                <text x={x + 22} y={y + 3.5} className="fill-foreground font-mono" fontSize={10}>
                  {span.id}
                </text>
              </motion.g>
              <rect x={tx(span.start)} y={y - 5} width={w} height={10} rx={2.5} fill="var(--faint)" />
              <motion.rect
                x={tx(span.start)}
                y={y - 5}
                height={10}
                rx={2.5}
                fill={span.depth === 0 ? 'var(--line-strong)' : 'var(--foreground)'}
                fillOpacity={span.depth === 0 ? 1 : 0.78}
                initial={live ? { width: 0 } : false}
                animate={{ width: w }}
                transition={{ duration: (span.end - span.start) * K, delay: span.start * K, ease: 'linear' }}
              />
            </g>
          )
        })}

        {/* Playhead */}
        {live ? (
          <motion.line
            y1={58}
            y2={rowY(ROWS.length - 1) + 10}
            stroke="var(--hero-accent)"
            strokeOpacity={0.5}
            initial={{ x1: X0, x2: X0, opacity: 1 }}
            animate={{ x1: tx(T), x2: tx(T), opacity: [1, 1, 0] }}
            transition={{
              x1: { duration: T * K, ease: 'linear' },
              x2: { duration: T * K, ease: 'linear' },
              opacity: { duration: T * K + 0.4, times: [0, 0.93, 1] },
            }}
          />
        ) : null}

        {/* Export over OTLP */}
        <g>
          <line x1={16} x2={444} y1={284} y2={284} stroke="var(--line)" />
          <text x={16} y={313} className="fill-foreground font-mono" fontSize={10}>
            OTLP
          </text>
          <motion.line
            x1={48}
            x2={96}
            y1={309.5}
            y2={309.5}
            stroke="var(--hero-accent)"
            initial={live ? { pathLength: 0 } : false}
            animate={{ pathLength: 1 }}
            transition={{ duration: 0.4, delay: EXPORT_AT }}
          />
          <path d="M 92 306 L 97 309.5 L 92 313" fill="none" stroke="var(--hero-accent)" />
          {TARGETS.map((name, i) => {
            const cx = 132 + i * 82
            return (
              <motion.g
                key={name}
                initial={live ? { opacity: 0.4 } : false}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.35, delay: EXPORT_AT + 0.25 + i * 0.12 }}
              >
                <rect
                  x={cx - 34}
                  y={299}
                  width={68}
                  height={21}
                  rx={6}
                  fill="var(--node)"
                  stroke={name === 'ADE' ? 'var(--hero-accent)' : 'var(--line-strong)'}
                />
                <text x={cx} y={313} textAnchor="middle" className="fill-foreground font-mono" fontSize={9.5}>
                  {name}
                </text>
              </motion.g>
            )
          })}
        </g>
      </g>
    </CoderFrame>
  )
}
