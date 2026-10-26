'use client'

import { AlignLeftIcon } from 'lucide-react'
import { motion, useMotionValue, useReducedMotion, useSpring, useTransform } from 'motion/react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'

import { duration, easeOut } from '@/lib/motion'
import styles from './article.module.css'

export type TocItem = { id: string; text: string; depth?: 2 | 3 }

export type TocGroup = {
  /** small label over the group; omit on the first group */
  label?: string
  items: TocItem[]
}

/** Headings at or above this line (the floating header plus a little room) count as passed. */
const LINE = 120

/** Where the line runs for each depth, in px from the list's left edge. Nested entries jog it to the right. */
const X: Record<2 | 3, number> = { 2: 6.5, 3: 22.5 }

/** How far into an entry's padding the line starts bending towards the next depth. */
const BEND = 10

type Entry = { id: string; top: number; bottom: number; depth: 2 | 3 }

/**
 * Builds the thread: one path that runs down the left of the list, hugging each entry at its depth's x and bending
 * smoothly where the depth changes, like a subway line. Straight runs stay straight so the dash-based progress
 * stroke reads cleanly.
 */
function thread(entries: Entry[]): string {
  if (!entries.length) return ''
  let d = `M${X[entries[0].depth]} ${entries[0].top}`
  for (let i = 0; i < entries.length; i++) {
    const e = entries[i]
    const next = entries[i + 1]
    const x = X[e.depth]
    if (!next || X[next.depth] === x) {
      d += ` V${next ? next.top : e.bottom}`
      continue
    }
    const nx = X[next.depth]
    const y0 = e.bottom - BEND
    const y1 = next.top + BEND
    const mid = (y0 + y1) / 2
    d += ` V${y0} C${x} ${mid} ${nx} ${mid} ${nx} ${y1}`
  }
  return d
}

/**
 * A contents list beside a long document, with a thread running down its left edge. A faint line traces every
 * entry as a dotted track, bending where the outline nests. As the reader scrolls, a bright stroke draws along the thread from the
 * top to the heading being read, and a small diamond rides its tip; both glide on a spring when the section
 * changes. The track fades in once when the page opens. The OS reduced-motion setting turns the glide and
 * the draw-in into plain jumps.
 */
export function TocRail({ title = 'On this page', groups }: { title?: string; groups: TocGroup[] }) {
  const key = groups.flatMap((g) => g.items.map((i) => i.id)).join('\n')
  const listRef = useRef<HTMLDivElement>(null)
  const pathRef = useRef<SVGPathElement>(null)
  const [entries, setEntries] = useState<Entry[]>([])
  const [active, setActive] = useState<string | null>(null)
  const [total, setTotal] = useState(0)
  const [marks, setMarks] = useState<number[]>([])
  const reduced = useReducedMotion()

  // Length of the bright stroke along the thread. The spring turns a change of section into a glide.
  const target = useMotionValue(0)
  const lit = useSpring(target, reduced ? { stiffness: 1200, damping: 100 } : { stiffness: 170, damping: 28 })
  const dashOffset = useTransform(lit, (v) => total - v)
  const tipX = useTransform(lit, (v) => pathRef.current?.getPointAtLength(v).x ?? 0)
  const tipY = useTransform(lit, (v) => pathRef.current?.getPointAtLength(v).y ?? 0)

  const d = thread(entries)

  // The list is the measuring stick: every link's box becomes one run of the thread.
  useLayoutEffect(() => {
    const list = listRef.current
    if (!list) return
    const measure = () => {
      const anchors = Array.from(list.querySelectorAll<HTMLAnchorElement>('a[data-toc-id]'))
      setEntries(
        anchors.map((a) => ({
          id: a.dataset.tocId ?? '',
          top: a.offsetTop,
          bottom: a.offsetTop + a.offsetHeight,
          depth: a.dataset.depth === '3' ? 3 : 2,
        })),
      )
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(list)
    return () => ro.disconnect()
  }, [])

  // Where along the thread each entry's centre line sits, found on the rendered path so the bends are exact.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `d` is derived from `entries` and is what the path draws
  useLayoutEffect(() => {
    const path = pathRef.current
    if (!path || !entries.length) return
    const length = path.getTotalLength()
    const at = (y: number) => {
      let lo = 0
      let hi = length
      for (let i = 0; i < 24; i++) {
        const mid = (lo + hi) / 2
        if (path.getPointAtLength(mid).y < y) lo = mid
        else hi = mid
      }
      return (lo + hi) / 2
    }
    setTotal(length)
    setMarks(entries.map((e) => at((e.top + e.bottom) / 2)))
  }, [d])

  useEffect(() => {
    if (!entries.length || !marks.length) return
    const headings = entries
      .map((e, i) => ({ id: e.id, mark: marks[i], el: document.getElementById(e.id) }))
      .filter((h): h is { id: string; mark: number; el: HTMLElement } => h.el !== null)
    if (!headings.length) return
    let frame = 0
    const pick = () => {
      frame = 0
      let index = 0
      for (let i = 0; i < headings.length; i++) {
        if (headings[i].el.getBoundingClientRect().top <= LINE) index = i
        else break
      }
      setActive(headings[index].id)
      target.set(headings[index].mark)
    }
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(pick)
    }
    pick()
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll)
    return () => {
      if (frame) cancelAnimationFrame(frame)
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onScroll)
    }
  }, [entries, marks, target])

  if (!key) return null

  const end = entries[entries.length - 1]

  return (
    <nav aria-label={title} className={styles.toc}>
      <p className={styles.tocTitle}>
        <AlignLeftIcon aria-hidden strokeWidth={1.75} className={styles.tocIcon} />
        <span>{title}</span>
      </p>
      <div ref={listRef} className={styles.tocList}>
        {entries.length ? (
          <svg className={styles.tocRail} width={30} height={end.bottom} aria-hidden="true">
            <motion.path
              ref={pathRef}
              className={styles.tocThread}
              d={d}
              initial={reduced ? false : { opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: duration.reveal, ease: easeOut }}
            />
            <motion.path
              className={styles.tocLit}
              d={d}
              strokeDasharray={total}
              style={{ strokeDashoffset: dashOffset }}
            />
            <circle className={styles.tocEnd} cx={X[end.depth]} cy={end.bottom} r={2.5} />
            <motion.g className={styles.tocTip} style={{ x: tipX, y: tipY }}>
              <rect x={-4} y={-4} width={8} height={8} rx={1.5} transform="rotate(45)" />
            </motion.g>
          </svg>
        ) : null}
        {groups.map((group, gi) => (
          <div key={group.label ?? gi}>
            {group.label ? <p className={styles.tocGroupLabel}>{group.label}</p> : null}
            <ul>
              {group.items.map((item) => {
                const current = item.id === active
                return (
                  <li key={item.id}>
                    <a
                      href={`#${item.id}`}
                      aria-current={current ? 'location' : undefined}
                      data-toc-id={item.id}
                      data-depth={item.depth ?? 2}
                      className={styles.tocLink}
                    >
                      {item.text}
                    </a>
                  </li>
                )
              })}
            </ul>
          </div>
        ))}
      </div>
    </nav>
  )
}
