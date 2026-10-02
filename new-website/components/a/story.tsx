'use client'

import { AnimatePresence, motion, useInView } from 'motion/react'
import { useEffect, useRef, useState } from 'react'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { SectionRule } from '@/components/site/section-rule'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { story } from './content'
import { CLOCK, type Stage } from './graph/model'
import { GraphScroller } from './graph/scroller'
import { captionFor, SystemGraph } from './graph/system-graph'
import { useStageClock } from './graph/use-stage-clock'
import { wideContainer } from './section'
import {
  type AsideProps,
  ComposeAside,
  DiscoverAside,
  ExtendAside,
  ObserveAside,
  ReactAside,
  type StoryCode,
  SummaryAside,
} from './story-asides'

type StoryStage = Extract<Stage, 'compose' | 'observe' | 'discover' | 'extend' | 'react' | 'summary'>

type Step = {
  stage: StoryStage
  letter: string
  name: string
  title: string
  subtitle: string
  tagline?: string
  solution?: string
  Aside: React.ComponentType<AsideProps>
}

/* Order follows the acronym. Discoverability ends with "write it yourself", which Extensibility picks up. */
const STEPS: Step[] = [
  { stage: 'compose', ...story.compose, Aside: ComposeAside },
  { stage: 'observe', ...story.observe, Aside: ObserveAside },
  { stage: 'discover', ...story.discover, Aside: DiscoverAside },
  { stage: 'extend', ...story.extend, Aside: ExtendAside },
  { stage: 'react', ...story.react, Aside: ReactAside },
  {
    stage: 'summary',
    letter: '',
    name: 'C.O.D.E.R.',
    title: story.summary.title,
    subtitle: story.summary.subtitle,
    Aside: SummaryAside,
  },
]

const LABELS: Record<StoryStage, string> = {
  compose:
    'Two compose files, one for local workers and one for the GPU box, group the workers on the graph. Both connect to the same engine, a GPU worker calls a local one, and a chain of functions runs across four workers.',
  observe:
    'Trace spans grow inside each worker as the request runs, with durations, ending in one trace of eight spans that is exported over OTLP.',
  discover:
    'The harness reads a prompt, finds github, browser and llm already in the system, searches workers.iii.dev for slack, installs it, and decides to write the missing digest worker itself.',
  extend:
    'The digest worker the harness wrote joins through the Worker SDK. Then a camera device does the same, exposing capture, stream and move functions the agent can call.',
  react:
    'github::pr::watch emits events. digest::format and slack::post observe them, and agent::review attaches as a new observer without changing the source.',
  summary:
    'The full graph with its five properties labelled: Composability, Observability, Discoverability, Extensibility, Reactivity.',
}

/** Story headings hold to two lines in the 40% column at every desktop width. */
const storyTitleClass =
  'mt-4 text-[32px] leading-[1.1] tracking-[-0.01em] sm:mt-5 sm:text-[36px] lg:text-[28px] xl:text-[34px] 2xl:text-[38px]'

/**
 * Sections 4 to 9. One graph, pinned on the left, grows through them while the steps scroll on the right.
 * On phones every step carries its own graph, so the story reads as separate sections.
 */
export function Story({ code }: { code: StoryCode }) {
  const [stage, setStage] = useState<StoryStage>('compose')
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const { step, cycle } = useStageClock(stage, running)
  const current = STEPS.find((s) => s.stage === stage) ?? STEPS[0]
  const index = STEPS.indexOf(current)

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <section id="coder" aria-label="C.O.D.E.R." className="landing-section relative">
      <SectionRule />
      <div ref={ref} className={cn(wideContainer, 'lg:grid lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:gap-14')}>
        <div className="hidden lg:block">
          <div className="lg:sticky lg:top-24 lg:flex lg:h-[calc(100vh-7.5rem)] lg:flex-col lg:py-2">
            <StageHeader index={index} stage={stage} name={current.name}>
              <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
            </StageHeader>
            <motion.div
              className="graphic-stage flex min-h-0 flex-1 items-center py-6"
              animate={{ scale: stage === 'summary' ? 0.96 : 1 }}
              transition={{ duration: 0.8, ease: easeOut }}
            >
              <SystemGraph
                stage={stage}
                step={step}
                cycle={cycle}
                active={running}
                label={LABELS[stage]}
                className="w-full"
              />
            </motion.div>
            <Narration stage={stage} step={step} running={running} />
          </div>
        </div>

        <ol className="min-w-0">
          {STEPS.map((s, i) => {
            const active = s.stage === stage
            return (
              <StoryStep
                key={s.stage}
                step={s}
                active={active}
                beat={active ? step : -1}
                onEnter={setStage}
                code={code}
                graph={
                  <>
                    <StageHeader index={i} stage={s.stage} name={s.name} />
                    <div className="graphic-stage py-2">
                      <GraphScroller>
                        <SystemGraph
                          stage={s.stage}
                          step={active ? step : CLOCK[s.stage].steps - 1}
                          cycle={cycle}
                          active={active && running}
                          label={LABELS[s.stage]}
                          className="w-full"
                        />
                      </GraphScroller>
                    </div>
                    <Narration
                      stage={s.stage}
                      step={active ? step : CLOCK[s.stage].steps - 1}
                      running={active && running}
                    />
                  </>
                }
              />
            )
          })}
        </ol>
      </div>
    </section>
  )
}

function StageHeader({
  index,
  stage,
  name,
  children,
}: {
  index: number
  stage: StoryStage
  name: string
  children?: React.ReactNode
}) {
  return (
    <div className="flex h-11 items-center justify-between gap-4 border-b">
      <ol className="flex items-center gap-1.5 font-sans text-[12px]" aria-label="Properties">
        {STEPS.slice(0, 5).map((s, i) => (
          <li
            key={s.stage}
            aria-current={s.stage === stage ? 'step' : undefined}
            className={cn(
              'flex size-6 items-center justify-center rounded-md border transition-colors duration-300',
              i <= index ? 'border-line-strong text-foreground' : 'border-transparent text-muted-foreground/60',
              s.stage === stage ? 'bg-faint' : '',
            )}
          >
            {s.letter}
          </li>
        ))}
      </ol>
      <p className="min-w-0 flex-1 truncate text-center font-sans text-[12px] text-foreground uppercase tracking-[0.08em]">
        {name}
      </p>
      {children ? <div className="flex w-[120px] justify-end">{children}</div> : null}
    </div>
  )
}

/** What is happening right now: step counter, one line of narration, and a bar that fills over the beat. */
function Narration({ stage, step, running }: { stage: StoryStage; step: number; running: boolean }) {
  const { steps, ms } = CLOCK[stage]
  const text = captionFor(stage, step) ?? ''
  return (
    <div className="border-t pt-3">
      <div className="flex items-baseline gap-3">
        <span className="shrink-0 font-sans text-[12px] text-muted-foreground tabular-nums">
          {step + 1} / {steps}
        </span>
        <AnimatePresence mode="wait" initial={false}>
          <motion.p
            key={`${stage}-${step}`}
            className="min-w-0 truncate font-sans text-[13px] text-foreground"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.25, ease: easeOut }}
          >
            {text}
          </motion.p>
        </AnimatePresence>
      </div>
      <div className="mt-2 h-px w-full overflow-hidden bg-border">
        <motion.div
          key={`${stage}-${step}-${running}`}
          className="h-full origin-left bg-foreground/60"
          initial={{ scaleX: 0 }}
          animate={{ scaleX: running ? 1 : 0 }}
          transition={{ duration: running ? ms / 1000 : 0, ease: 'linear' }}
        />
      </div>
    </div>
  )
}

function StoryStep({
  step,
  active,
  beat,
  onEnter,
  code,
  graph,
}: {
  step: Step
  active: boolean
  beat: number
  onEnter: (stage: StoryStage) => void
  code: StoryCode
  graph: React.ReactNode
}) {
  const ref = useRef<HTMLLIElement>(null)
  const inView = useInView(ref, { margin: '-45% 0px -45% 0px' })
  useEffect(() => {
    if (inView) onEnter(step.stage)
  }, [inView, onEnter, step.stage])
  const id = `coder-${step.stage}`
  return (
    <li
      ref={ref}
      id={id}
      aria-current={active ? 'true' : undefined}
      className="flex min-w-0 flex-col justify-center py-12 sm:py-16 lg:min-h-screen lg:py-24"
    >
      <Reveal as="header">
        <p className="font-medium font-sans text-muted-foreground text-xs uppercase leading-none tracking-[0.08em]">
          {step.letter ? `${step.letter} · ${step.name}` : step.name}
        </p>
        <PixelHeading id={`${id}-title`} className={storyTitleClass}>
          {step.title}
        </PixelHeading>
        <p className="mt-4 text-pretty text-[17px] text-foreground leading-relaxed">{step.subtitle}</p>
        {step.tagline ? <p className="mt-3 font-sans text-[13px] text-hero-accent">{step.tagline}</p> : null}
        {step.solution ? (
          <p className="mt-3 text-pretty text-[15px] text-muted-foreground leading-relaxed">{step.solution}</p>
        ) : null}
      </Reveal>
      {/* Phones: this step's own graph */}
      <Reveal delay={0.05} className="mt-8 lg:hidden">
        {graph}
      </Reveal>
      <Reveal delay={0.08} className="mt-8">
        <step.Aside step={beat} code={code} />
      </Reveal>
    </li>
  )
}
