'use client'

import { AnimatePresence, motion } from 'motion/react'

import { CoderFrame, ease, StatusDot, SvgLabel, useCoderSteps } from './coder-kit'

const INSTALLED = [
  'tickets::create',
  'tickets::comment',
  'kanban::move',
  'repo::search',
  'sim::tap',
  'pg::query',
  'orders::refund',
]
const PUBLIC = ['diagrams@0.9.1', 'browser@1.8.0']

const L = { x: 16, w: 148 }
const RX = 180
const rowY = (i: number) => 72 + i * 22
const pubY = (i: number) => 280 + i * 22

type Event = { at: number; tone: 'ok' | 'warn'; left: string; right: string }
const EVENTS: Event[] = [
  { at: 2, tone: 'ok', left: '→ tickets::create', right: '200 · 41ms' },
  { at: 3, tone: 'warn', left: 'contract changed', right: 'pushed to model' },
  { at: 4, tone: 'ok', left: 'search "diagram"', right: '0 installed · 1 public' },
  { at: 6, tone: 'ok', left: 'install diagrams@0.9.1', right: 'running' },
  { at: 7, tone: 'ok', left: '→ diagram::render', right: '200 · 88ms' },
]

const CAPTIONS = [
  'agent searches the registry: "ticket"',
  'reads the contract: description + JSON schema',
  'calls tickets::create from inside the system',
  'contract changed mid-conversation → the harness tells the model',
  'nothing installed renders diagrams',
  'found in the public registry: diagrams@0.9.1',
  'installed and registered in the same turn',
  'calls diagram::render, same turn',
  'the registry is the tool list',
]

/**
 * Discoverability: an agent searches the registry, reads a function contract and calls it,
 * is told when that contract changes, then installs a missing capability and uses it in one turn.
 */
export function CoderDiscover() {
  const { ref, active, step } = useCoderSteps(CAPTIONS.length, 1000)
  const secondQuery = step >= 4
  const query = secondQuery ? 'diagram' : 'ticket'
  const installed = step >= 6
  const changed = step >= 3
  const card = step >= 1 && step <= 3 ? 'ticket' : step >= 5 ? 'diagram' : null
  const matches = (id: string) => !secondQuery && step <= 3 && id.startsWith('tickets::')

  return (
    <CoderFrame
      frameRef={ref}
      label="An agent searches the registry, which is also its tool list. It reads the contract for tickets::create, with a description and JSON schema, and calls it. The contract gains a field and the change is pushed to the model. A diagram capability is missing, so the agent installs diagrams from the public registry and calls diagram::render in the same turn."
      caption={CAPTIONS[step]}
      captionKey={step}
    >
      {/* Registry = tool list */}
      <rect x={L.x} y={12} width={L.w} height={316} rx={10} fill="var(--node)" stroke="var(--line-strong)" />
      <SvgLabel x={L.x + 12} y={30}>
        REGISTRY
      </SvgLabel>
      <text x={L.x + 12} y={44} className="fill-muted-foreground font-mono" fontSize={9}>
        = the model&apos;s tool list
      </text>
      <line x1={L.x} x2={L.x + L.w} y1={54} y2={54} stroke="var(--line)" />

      {INSTALLED.map((id, i) => {
        const hit = active && matches(id)
        const flash = active && step === 3 && id === 'tickets::create'
        return (
          <g key={id}>
            <rect
              x={L.x + 6}
              y={rowY(i) - 9}
              width={L.w - 12}
              height={18}
              rx={5}
              fill={hit || flash ? 'var(--faint)' : 'transparent'}
              stroke={flash ? 'var(--warn)' : hit ? 'var(--hero-accent)' : 'transparent'}
              style={{ transition: 'stroke 300ms ease, fill 300ms ease' }}
            />
            <text x={L.x + 14} y={rowY(i) + 3.5} className="fill-foreground font-mono" fontSize={10}>
              {id}
            </text>
          </g>
        )
      })}
      <motion.g
        initial={false}
        animate={{ opacity: installed ? 1 : 0, x: installed ? 0 : -8 }}
        transition={{ duration: 0.5, delay: installed && active ? 0.3 : 0, ease }}
      >
        <rect
          x={L.x + 6}
          y={rowY(INSTALLED.length) - 9}
          width={L.w - 12}
          height={18}
          rx={5}
          fill="var(--faint)"
          stroke={active && step === 7 ? 'var(--hero-accent)' : 'var(--line)'}
        />
        <text x={L.x + 14} y={rowY(INSTALLED.length) + 3.5} className="fill-foreground font-mono" fontSize={10}>
          diagram::render
        </text>
      </motion.g>

      <line x1={L.x} x2={L.x + L.w} y1={250} y2={250} stroke="var(--line)" strokeDasharray="3 4" />
      <SvgLabel x={L.x + 12} y={266}>
        PUBLIC
      </SvgLabel>
      {PUBLIC.map((pkg, i) => {
        const hit = active && pkg.startsWith('diagrams') && (step === 4 || step === 5)
        const done = pkg.startsWith('diagrams') && installed
        return (
          <g key={pkg} opacity={done ? 0.45 : 1} style={{ transition: 'opacity 400ms ease' }}>
            <rect
              x={L.x + 6}
              y={pubY(i) - 9}
              width={L.w - 12}
              height={18}
              rx={5}
              fill="transparent"
              stroke={hit ? 'var(--hero-accent)' : 'var(--line)'}
              strokeDasharray="3 3"
              style={{ transition: 'stroke 300ms ease' }}
            />
            <text x={L.x + 14} y={pubY(i) + 3.5} className="fill-muted-foreground font-mono" fontSize={10}>
              {pkg}
            </text>
            {done ? <circle cx={L.x + L.w - 16} cy={pubY(i)} r={2.5} fill="var(--ok)" /> : null}
          </g>
        )
      })}

      {/* Search */}
      <rect x={RX} y={12} width={444 - RX} height={28} rx={8} fill="var(--node)" stroke="var(--line-strong)" />
      <circle cx={RX + 15} cy={25} r={4} fill="none" stroke="var(--muted-foreground)" />
      <line x1={RX + 18} y1={28} x2={RX + 21} y2={31} stroke="var(--muted-foreground)" />
      <text x={RX + 28} y={30} className="fill-muted-foreground font-mono" fontSize={10.5}>
        search
      </text>
      <AnimatePresence mode="wait" initial={false}>
        <motion.text
          key={query}
          x={RX + 72}
          y={30}
          className="fill-foreground font-mono"
          fontSize={10.5}
          initial={{ opacity: 0, x: -6 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.3, ease }}
        >
          &quot;{query}&quot;
        </motion.text>
      </AnimatePresence>
      <text x={432} y={30} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={9}>
        installed + public
      </text>

      {/* Contract */}
      <AnimatePresence mode="wait" initial={false}>
        {card ? (
          <motion.g
            key={card}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.4, ease }}
          >
            <rect
              x={RX}
              y={52}
              width={444 - RX}
              height={118}
              rx={10}
              fill="var(--node)"
              stroke={active && (step === 2 || step === 7) ? 'var(--hero-accent)' : 'var(--line-strong)'}
              style={{ transition: 'stroke 300ms ease' }}
            />
            <text x={RX + 12} y={72} className="fill-foreground font-mono" fontSize={11}>
              {card === 'ticket' ? 'tickets::create' : 'diagram::render'}
            </text>
            <text x={432} y={72} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={8.5}>
              {card === 'ticket' || installed ? 'installed' : 'public · diagrams@0.9.1'}
            </text>
            <text x={RX + 12} y={89} className="fill-muted-foreground" fontSize={10.5}>
              {card === 'ticket' ? 'Open a ticket on the board.' : 'Render Mermaid source to SVG.'}
            </text>
            <line x1={RX} x2={444} y1={99} y2={99} stroke="var(--line)" />
            {card === 'ticket' ? (
              <g className="font-mono" fontSize={9.5}>
                <text x={RX + 12} y={117} className="fill-muted-foreground">
                  in
                </text>
                <text x={RX + 38} y={117} className="fill-foreground">
                  {'{ title: string,'}
                </text>
                <text x={RX + 38} y={132} className="fill-foreground">
                  {changed ? '  priority?: "low" | "high",' : '  priority?: "low" | "high" }'}
                </text>
                <motion.g
                  initial={false}
                  animate={{ opacity: changed ? 1 : 0, x: changed ? 0 : -6 }}
                  transition={{ duration: 0.45, ease }}
                >
                  <rect x={RX + 34} y={137} width={124} height={15} rx={4} fill="var(--faint)" stroke="var(--warn)" />
                  <text x={RX + 38} y={147.5} className="fill-foreground">
                    {'+ assignee: string }'}
                  </text>
                </motion.g>
                <text x={RX + 12} y={163} className="fill-muted-foreground">
                  out
                </text>
                <text x={RX + 38} y={163} className="fill-foreground">
                  {'{ id: string, url: string }'}
                </text>
              </g>
            ) : (
              <g className="font-mono" fontSize={9.5}>
                <text x={RX + 12} y={117} className="fill-muted-foreground">
                  in
                </text>
                <text x={RX + 38} y={117} className="fill-foreground">
                  {'{ source: string }'}
                </text>
                <text x={RX + 12} y={132} className="fill-muted-foreground">
                  out
                </text>
                <text x={RX + 38} y={132} className="fill-foreground">
                  {'{ svg: string }'}
                </text>
                {!installed ? (
                  <g>
                    <rect x={RX + 12} y={143} width={58} height={18} rx={5} fill="var(--foreground)" />
                    <text
                      x={RX + 41}
                      y={155.5}
                      textAnchor="middle"
                      className="fill-background font-mono"
                      fontSize={9.5}
                    >
                      install
                    </text>
                  </g>
                ) : (
                  <g>
                    <circle cx={RX + 16} cy={152} r={2.75} fill="var(--ok)" />
                    <text x={RX + 24} y={155.5} className="fill-muted-foreground font-mono" fontSize={9.5}>
                      registered · callable now
                    </text>
                  </g>
                )}
              </g>
            )}
          </motion.g>
        ) : (
          <motion.g
            key="searching"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.3 }}
          >
            <rect
              x={RX}
              y={52}
              width={444 - RX}
              height={118}
              rx={10}
              fill="none"
              stroke="var(--line)"
              strokeDasharray="3 4"
            />
            <text
              x={(RX + 444) / 2}
              y={115}
              textAnchor="middle"
              className="fill-muted-foreground font-mono"
              fontSize={10}
            >
              {secondQuery ? 'no installed match' : 'searching…'}
            </text>
          </motion.g>
        )}
      </AnimatePresence>

      {/* Turn log */}
      <SvgLabel x={RX} y={192}>
        AGENT TURN
      </SvgLabel>
      {EVENTS.map((e, i) => {
        const y = 212 + i * 24
        const shown = step >= e.at
        return (
          <motion.g
            key={e.left}
            initial={false}
            animate={{ opacity: shown ? 1 : 0, y: shown ? 0 : 4 }}
            transition={{ duration: 0.4, ease }}
          >
            <line x1={RX} x2={444} y1={y + 12} y2={y + 12} stroke="var(--line)" />
            <circle cx={RX + 4} cy={y} r={2.75} fill={`var(--${e.tone})`} />
            <text x={RX + 14} y={y + 3.5} className="fill-foreground font-mono" fontSize={10}>
              {e.left}
            </text>
            <text x={444} y={y + 3.5} textAnchor="end" className="fill-muted-foreground font-mono" fontSize={9}>
              {e.right}
            </text>
          </motion.g>
        )
      })}

      {active && (step === 2 || step === 7) ? <StatusDot key={step} cx={432} cy={86} /> : null}
    </CoderFrame>
  )
}
