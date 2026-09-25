"use client"

import { useEffect, useId, useRef } from "react"
import { cn } from "@/lib/utils"
import styles from "./graph.module.css"
import {
  ACTUAL_EDGES,
  CENTER,
  CHIP_H,
  CHIP_R,
  callPaths,
  chordPath,
  type Edge,
  HUB_R,
  MARK_BOX,
  MARK_RECTS,
  MARK_SIZE,
  MESH_EDGES,
  NODES,
  PACKET_BASE_MS,
  PACKET_JITTER_MS,
  PACKET_RADIUS,
  STAGES,
  type Stage,
  spokePath,
  VIEWBOX,
} from "./viz-data"

const SVG_NS = "http://www.w3.org/2000/svg"
const EASE_IN_OUT = "cubic-bezier(0.65, 0, 0.35, 1)"
const EASE_OUT = "cubic-bezier(0.2, 0.8, 0.3, 1)"
const EASE_IN = "cubic-bezier(0.4, 0, 1, 1)"

const isActual = new Set(ACTUAL_EDGES.map(([i, j]) => `${i}-${j}`))
const pick = (edges: Edge[]) => edges[Math.floor(Math.random() * edges.length)]

// Every chord, drawn once; which ones show is the stylesheet's call per stage.
// The stagger index walks around the ring so a stage draws in as a sweep.
const CHORDS = MESH_EDGES.map(([i, j], k) => ({
  key: `${i}-${j}`,
  d: chordPath(i, j),
  actual: isActual.has(`${i}-${j}`),
  order: k,
}))

type GraphProps = {
  stage: Stage
  /** false stops spawning calls (off screen, background tab or reduced motion) */
  running: boolean
}

function make<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string>, parent: Element) {
  const el = document.createElementNS(SVG_NS, tag)
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
  parent.appendChild(el)
  return el
}

/**
 * The service graph. Static drawing on the server (ring, chords, spokes, the
 * engine at the hub); the stage decides what is drawn in, via `data-stage`
 * and the stylesheet. On the client a loop spawns calls. In the first two
 * stages a light rides a chord. In the iii stage a call goes through the
 * engine: a leg draws from the caller to the hub's edge, the hub's ring
 * breathes as the call lands, and a second leg draws out to the callee. The
 * light rides both legs and is hidden while it is inside the engine.
 */
export function Graph({ stage, running }: GraphProps) {
  const packetsRef = useRef<SVGGElement>(null)
  const callsRef = useRef<SVGGElement>(null)
  const glowId = useId()
  // The loop reads the stage through a ref so a stage change never restarts it.
  const stageRef = useRef(stage)
  useEffect(() => {
    stageRef.current = stage
  }, [stage])

  useEffect(() => {
    const packetLayer = packetsRef.current
    const callLayer = callsRef.current
    if (!running || !packetLayer || !callLayer) return

    let inFlight = 0
    let lastSpawn = 0
    let raf = 0
    const live = new Set<Animation>()

    /** Runs `anim`, removes `el` when it ends, and only counts the call done when `counts` is set. */
    const watch = (anim: Animation, el: Element, counts = false) => {
      live.add(anim)
      const done = () => {
        live.delete(anim)
        el.remove()
        if (counts) inFlight--
      }
      anim.finished.then(done).catch(done)
    }

    const ride = (d: string, keyframes: Keyframe[], options: KeyframeAnimationOptions) => {
      const dot = make("circle", { r: String(PACKET_RADIUS), class: styles.packet }, packetLayer)
      dot.style.offsetPath = `path("${d}")`
      return { dot, anim: dot.animate(keyframes, options) }
    }

    const spawnChord = (from: number, to: number, duration: number) => {
      const { dot, anim } = ride(
        chordPath(from, to),
        [
          { offsetDistance: "0%", opacity: 0 },
          { offsetDistance: "8%", opacity: 1, offset: 0.08 },
          { offsetDistance: "92%", opacity: 1, offset: 0.92 },
          { offsetDistance: "100%", opacity: 0 },
        ],
        { duration, easing: EASE_IN_OUT, fill: "forwards" },
      )
      watch(anim, dot, true)
    }

    const spawnCall = (from: number, to: number, duration: number) => {
      const { legIn, legOut, ride: path, split } = callPaths(from, to)
      // The whole call runs 30% longer than a chord trip: the lines linger after the light lands.
      const total = duration * 1.3
      const land = 1 / 1.3
      const inEnd = split * 0.86 // the light reaches the hub
      const outStart = split * 1.14 // and leaves it

      // Leg in: draws behind the light, holds while the hub takes the call, then fades.
      const a = make("path", { d: legIn, pathLength: "1", class: styles.call }, callLayer)
      watch(
        a.animate(
          [
            { strokeDashoffset: 1, opacity: 0.9, easing: EASE_IN },
            { strokeDashoffset: 0, opacity: 0.9, offset: inEnd },
            { strokeDashoffset: 0, opacity: 0.9, offset: outStart, easing: "ease" },
            { strokeDashoffset: 0, opacity: 0 },
          ],
          { duration: total, fill: "forwards" },
        ),
        a,
      )

      // Leg out: starts as the light leaves the hub, draws ahead to the callee, fades after arrival.
      const b = make("path", { d: legOut, pathLength: "1", class: styles.call }, callLayer)
      watch(
        b.animate(
          [
            { strokeDashoffset: 1, opacity: 0.9 },
            { strokeDashoffset: 1, opacity: 0.9, offset: outStart, easing: EASE_OUT },
            { strokeDashoffset: 0, opacity: 0.9, offset: land, easing: "ease" },
            { strokeDashoffset: 0, opacity: 0 },
          ],
          { duration: total, fill: "forwards" },
        ),
        b,
      )

      // The hub's ring breathes out once as the call lands.
      const flash = make(
        "circle",
        { cx: String(CENTER.x), cy: String(CENTER.y), r: String(HUB_R), class: styles.flash },
        callLayer,
      )
      watch(
        flash.animate(
          [
            { opacity: 0, transform: "scale(1)" },
            { opacity: 0, transform: "scale(1)", offset: inEnd - 0.04 },
            { opacity: 0.7, transform: "scale(1.06)", offset: split, easing: EASE_OUT },
            { opacity: 0, transform: "scale(1.22)", offset: split + 0.3, easing: EASE_OUT },
            { opacity: 0, transform: "scale(1.22)" },
          ],
          { duration: total, fill: "forwards" },
        ),
        flash,
      )

      // The light: eases into the hub, vanishes inside, reappears on the far edge.
      const atHub = `${split * 100}%`
      const { dot, anim } = ride(
        path,
        [
          { offsetDistance: "0%", opacity: 0 },
          { offsetDistance: "6%", opacity: 1, offset: 0.06, easing: EASE_IN },
          { offsetDistance: atHub, opacity: 1, offset: inEnd },
          { offsetDistance: atHub, opacity: 0, offset: inEnd + 0.03 },
          { offsetDistance: atHub, opacity: 0, offset: outStart - 0.03 },
          { offsetDistance: atHub, opacity: 1, offset: outStart, easing: EASE_OUT },
          { offsetDistance: "94%", opacity: 1, offset: land - 0.04 },
          { offsetDistance: "100%", opacity: 0, offset: land },
          { offsetDistance: "100%", opacity: 0 },
        ],
        { duration: total, fill: "forwards" },
      )
      watch(anim, dot, true)
    }

    const spawn = () => {
      const current = stageRef.current
      const edge = pick(current === "actual" ? ACTUAL_EDGES : MESH_EDGES)
      const [from, to] = Math.random() < 0.5 ? edge : [edge[1], edge[0]]
      const duration = PACKET_BASE_MS + Math.random() * PACKET_JITTER_MS
      inFlight++
      if (current === "iii") spawnCall(from, to, duration)
      else spawnChord(from, to, duration)
    }

    const tick = (now: number) => {
      const { spawnEvery, maxPackets } = STAGES[stageRef.current]
      if (now - lastSpawn > spawnEvery && inFlight < maxPackets) {
        spawn()
        lastSpawn = now
      }
      raf = requestAnimationFrame(tick)
    }

    raf = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(raf)
      for (const anim of live) anim.cancel()
      live.clear()
      packetLayer.replaceChildren()
      callLayer.replaceChildren()
    }
  }, [running])

  const markScale = MARK_SIZE / MARK_BOX
  const markOffset = MARK_SIZE / 2

  return (
    <svg
      viewBox={`0 0 ${VIEWBOX} ${VIEWBOX}`}
      preserveAspectRatio="xMidYMid meet"
      aria-hidden="true"
      data-stage={stage}
      className={cn(styles.svg, "absolute inset-0 block size-full overflow-visible")}
    >
      <defs>
        {/* Soft light behind the engine, fading to nothing. */}
        <radialGradient id={glowId}>
          <stop offset="0" style={{ stopColor: "var(--gray-12)", stopOpacity: 0.16 }} />
          <stop offset="0.55" style={{ stopColor: "var(--gray-12)", stopOpacity: 0.05 }} />
          <stop offset="1" style={{ stopColor: "var(--gray-12)", stopOpacity: 0 }} />
        </radialGradient>
      </defs>

      {/* Chords, all 45, each one dash the length of its path; the stage decides which are drawn in. */}
      <g>
        {CHORDS.map((c) => (
          <path
            key={c.key}
            d={c.d}
            pathLength={1}
            data-actual={c.actual ? "" : undefined}
            className={styles.edge}
            style={{ "--i": c.order } as React.CSSProperties}
          />
        ))}
      </g>

      {/* Spokes: every service plugged into the engine (iii stage). */}
      <g>
        {NODES.map((n, i) => (
          <path
            key={n.label}
            d={spokePath(i)}
            pathLength={1}
            className={styles.spoke}
            style={{ "--i": i } as React.CSSProperties}
          />
        ))}
      </g>

      {/* PostHog session replay blocks these churny layers by id, so the ids must stay static. */}
      {/* biome-ignore lint/correctness/useUniqueElementIds: fixed id referenced by the PostHog block list */}
      <g id="hv-edges-ephemeral" ref={callsRef} />

      {/* The engine at the hub (iii stage only): glow, disc with a bright ring, the mark. */}
      <g className={styles.hub}>
        <circle cx={CENTER.x} cy={CENTER.y} r={HUB_R * 2.6} fill={`url(#${glowId})`} />
        <circle cx={CENTER.x} cy={CENTER.y} r={HUB_R} className={styles.hubDisc} />
        <g
          className={styles.hubMark}
          transform={`translate(${CENTER.x - markOffset} ${CENTER.y - markOffset}) scale(${markScale})`}
        >
          {MARK_RECTS.map((r) => (
            <rect key={`${r.x}-${r.y}`} {...r} />
          ))}
        </g>
      </g>

      {/* biome-ignore lint/correctness/useUniqueElementIds: fixed id referenced by the PostHog block list */}
      <g id="hv-packets" ref={packetsRef} />

      {/* Services on the ring, as chips painted over the edges: a status dot and the
          label inside a rounded rect. Lines end at the chip border by paint order. */}
      <g>
        {NODES.map((n) => {
          const left = n.x - n.w / 2
          return (
            <g key={n.label}>
              <rect x={left} y={n.y - CHIP_H / 2} width={n.w} height={CHIP_H} rx={CHIP_R} className={styles.chip} />
              <circle cx={left + 12} cy={n.y} r={2.5} className={styles.status} />
              <text x={left + 21} y={n.y} dominantBaseline="central" className={styles.label}>
                {n.label}
              </text>
            </g>
          )
        })}
      </g>
    </svg>
  )
}
