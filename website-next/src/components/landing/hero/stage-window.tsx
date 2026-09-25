"use client"

import { AnimatePresence, animate, m, useInView, useMotionValue, useReducedMotion, useTransform } from "motion/react"
import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react"
import { LogoMark } from "@/components/site/logo"
import { usePageVisible } from "@/hooks/use-page-visible"
import { track } from "@/lib/analytics"
import { cn } from "@/lib/utils"
import { ComplexityFormula } from "./complexity-formula"
import { Graph } from "./graph"
import { StageTabs } from "./stage-tabs"
import { CHART_BASE, CHART_H, CHART_VIEWBOX, CHART_W, CURVES, NODES, STAGE_ORDER, STAGES, type Stage } from "./viz-data"

const EASE_OUT = [0.23, 1, 0.32, 1] as const

/**
 * "From quadratic to linear to ephemeral", as a product window: the live
 * service graph on the left, the stage's story on the right. Stages advance
 * on their own (a linear progress line shows the time left) until the visitor
 * picks one; everything pauses off screen and in background tabs.
 */
export function StageWindow() {
  const ref = useRef<HTMLElement>(null)
  const inView = useInView(ref, { amount: 0.15 })
  // Reduced motion only after hydration: the server can't know the OS setting,
  // so applying it during the first client render would mismatch the markup.
  const hydrated = useSyncExternalStore(
    () => () => undefined,
    () => true,
    () => false,
  )
  const reduce = (useReducedMotion() ?? false) && hydrated
  const [stage, setStage] = useState<Stage>("mesh")
  const [pinned, setPinned] = useState(false)
  const tabVisible = usePageVisible()

  const autoplay = !pinned && !reduce
  const playing = autoplay && inView && tabVisible
  const config = STAGES[stage]
  const step = STAGE_ORDER.indexOf(stage)

  function next() {
    setStage((s) => STAGE_ORDER[(STAGE_ORDER.indexOf(s) + 1) % STAGE_ORDER.length])
  }

  function select(s: Stage) {
    track("tab_switch", { tab_group: "hero_viz", tab_id: s, tab_label: STAGES[s].tab })
    setPinned(true)
    setStage(s)
  }

  return (
    <figure
      ref={ref}
      aria-label="From quadratic to linear to ephemeral"
      className="relative overflow-hidden rounded-[18px] bg-gray-2 text-left shadow-panel"
    >
      {/* Window chrome */}
      <div className="relative grid h-14 grid-cols-[1fr_auto_1fr] items-center gap-4 border-b border-gray-5 px-4 max-sm:grid-cols-1 max-sm:px-3 sm:px-5">
        <div aria-hidden="true" className="flex gap-1.5 max-sm:hidden">
          <span className="size-2.5 rounded-full bg-gray-6" />
          <span className="size-2.5 rounded-full bg-gray-6" />
          <span className="size-2.5 rounded-full bg-gray-6" />
        </div>
        <StageTabs stage={stage} onSelect={select} className="max-sm:w-full" />
        <span className="justify-self-end text-[12px] text-gray-10 tabular-nums max-sm:hidden">
          n = {NODES.length} services
        </span>

        {/* Time left in this stage. Linear, because it's a clock; its end advances the stage. */}
        {autoplay && (
          <span
            key={stage}
            aria-hidden="true"
            onAnimationEnd={next}
            style={{
              animationDuration: `${config.dwell}ms`,
              animationPlayState: playing ? "running" : "paused",
            }}
            className="absolute inset-x-0 bottom-[-1px] h-px origin-left animate-[stage-progress_linear_forwards] bg-gray-12/60"
          />
        )}
      </div>

      <div className="grid md:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
        {/* Graph */}
        <div className="relative aspect-square overflow-hidden border-gray-5 max-md:border-b md:aspect-auto md:h-[540px] md:border-r">
          <div className="absolute inset-4 sm:inset-8">
            <Graph stage={stage} running={inView && tabVisible && !reduce} />
          </div>
        </div>

        {/* Story */}
        <div className="flex min-h-[420px] flex-col p-6 sm:p-8">
          <p className="text-[12px] font-medium tracking-[0.06em] text-gray-10 uppercase tabular-nums">
            0{step + 1} <span className="text-gray-8">/ 0{STAGE_ORDER.length}</span>
          </p>

          <AnimatePresence mode="popLayout" initial={false}>
            <m.div
              key={stage}
              initial={reduce ? { opacity: 0 } : { opacity: 0, y: 8, filter: "blur(4px)" }}
              animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
              exit={
                reduce
                  ? { opacity: 0, transition: { duration: 0.12 } }
                  : { opacity: 0, y: -4, filter: "blur(4px)", transition: { duration: 0.16, ease: [0.4, 0, 1, 1] } }
              }
              transition={{ duration: 0.4, ease: EASE_OUT }}
              className="mt-3"
            >
              {/* The resolution carries the mark: this is the stage that is ours. */}
              <h3 className="flex h-8 items-center gap-2.5 text-2xl font-semibold tracking-[-0.025em] text-gray-12">
                {stage === "iii" && <LogoMark className="size-6" />}
                {config.name}
              </h3>
              <p className="mt-3 text-[15px] leading-[1.65] tracking-[-0.006em] text-gray-11">
                {config.body.map((part) =>
                  part.em ? (
                    <span key={part.text} className="font-medium text-gray-12">
                      {part.text}
                    </span>
                  ) : (
                    <span key={part.text}>{part.text}</span>
                  ),
                )}
              </p>
            </m.div>
          </AnimatePresence>

          <div className="mt-auto pt-8">
            <div className="flex items-end justify-between gap-6">
              <div>
                <IntegrationCount value={config.count} accent={config.accentCount} instant={reduce} />
                <p className="mt-1 text-[13px] text-gray-10">integrations</p>
              </div>
              <div className="min-h-[44px] text-right text-[18px] text-gray-12 [&_math]:text-[18px] [&_math]:[font-family:inherit]">
                <AnimatePresence mode="popLayout" initial={false}>
                  <m.div
                    key={stage}
                    initial={{ opacity: 0, filter: "blur(4px)" }}
                    animate={{ opacity: 1, filter: "blur(0px)" }}
                    exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.14 } }}
                    transition={{ duration: 0.3, ease: EASE_OUT }}
                  >
                    <ComplexityFormula stage={stage} />
                  </m.div>
                </AnimatePresence>
              </div>
            </div>
            <ComplexityChart stage={stage} reduce={reduce} />
          </div>
        </div>
      </div>
    </figure>
  )
}

/** Tabular counter that tweens between stage totals (45 → 20 → 0). No layout shift. */
function IntegrationCount({ value, accent, instant }: { value: number; accent: boolean; instant: boolean }) {
  const count = useMotionValue(value)
  const rounded = useTransform(count, (v) => Math.round(v))

  useEffect(() => {
    if (instant) {
      count.jump(value)
      return
    }
    const controls = animate(count, value, { duration: 0.8, ease: EASE_OUT })
    return () => controls.stop()
  }, [count, value, instant])

  return (
    <m.span
      className={cn(
        "block text-[44px] leading-none font-semibold tracking-[-0.04em] text-gray-12 tabular-nums",
        accent && "opacity-100",
      )}
    >
      {rounded}
    </m.span>
  )
}

/** The draw-in: fast start, long settle. From the better-svg reference values. */
const DRAW = { duration: 0.9, ease: [0.2, 0.8, 0.3, 1] as const }

/**
 * Integration cost against services, per stage. The three curves sit faint as
 * reference. On each stage change the active one is drawn in from the origin
 * (pathLength 0 → 1), a fill blooms under it as it lands, and a dot marks the
 * end. Keyed by stage so the draw replays on every switch.
 */
function ComplexityChart({ stage, reduce }: { stage: Stage; reduce: boolean }) {
  const glowId = useId()
  const active = CURVES.find((c) => c.stage === stage) ?? CURVES[0]

  return (
    <svg
      viewBox={CHART_VIEWBOX}
      aria-hidden="true"
      className="mt-5 block aspect-[4/1] w-full overflow-visible"
      style={{ transformBox: "fill-box" }}
    >
      <defs>
        <linearGradient id={glowId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: "var(--gray-12)", stopOpacity: 0.14 }} />
          <stop offset="1" style={{ stopColor: "var(--gray-12)", stopOpacity: 0 }} />
        </linearGradient>
      </defs>

      {/* Axes */}
      <line x1={0} y1={CHART_BASE} x2={CHART_W} y2={CHART_BASE} className="stroke-gray-6" strokeWidth={1} />
      <line x1={0} y1={CHART_BASE} x2={0} y2={8} className="stroke-gray-6" strokeWidth={1} />

      {/* Reference curves, always present, faint. */}
      <g className="fill-none stroke-gray-7" strokeWidth={1} strokeLinecap="round">
        {CURVES.map((c) => (
          <path key={c.id} d={c.d} />
        ))}
      </g>

      {/* The active curve, redrawn on every stage change. */}
      <AnimatePresence initial={false}>
        <m.g key={stage} exit={{ opacity: 0, transition: { duration: 0.18, ease: [0.4, 0, 1, 1] } }}>
          <m.path
            d={active.area}
            fill={`url(#${glowId})`}
            initial={reduce ? false : { opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.6, delay: DRAW.duration * 0.55, ease: "easeOut" }}
          />
          <m.path
            d={active.d}
            className="fill-none stroke-gray-12"
            strokeWidth={1.75}
            strokeLinecap="round"
            initial={reduce ? false : { pathLength: 0 }}
            animate={{ pathLength: 1 }}
            transition={DRAW}
          />
          <m.circle
            cx={active.end.x}
            cy={active.end.y}
            r={3}
            className="fill-gray-12"
            style={{ transformBox: "fill-box", transformOrigin: "center" }}
            initial={reduce ? false : { opacity: 0, scale: 0.6 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ duration: 0.3, delay: DRAW.duration * 0.85, ease: [0.2, 0.8, 0.3, 1] }}
          />
        </m.g>
      </AnimatePresence>
      {/* Keep the viewBox height in the layout even when nothing else references it. */}
      <rect x={0} y={0} width={CHART_W} height={CHART_H} fill="none" />
    </svg>
  )
}
