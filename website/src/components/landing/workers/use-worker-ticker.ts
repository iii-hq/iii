"use client"

import { type MotionValue, useMotionValue } from "motion/react"
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react"
import { type CardCommand, startTicker, type TickerCard } from "./ticker-runner"

export type { CardCommand, TickerCard } from "./ticker-runner"

type Options = {
  /** the ticker has entered the viewport at least once */
  started: boolean
  /** the ticker is on screen; the sequence pauses between steps when false */
  active: boolean
  reducedMotion: boolean
}

/**
 * Drives the workers showcase: types a search term into the registry search
 * field, then glides a strip of worker cards until the matching worker sits
 * centred and highlighted, and types its install command. The sequence itself
 * runs in `startTicker`; this hook owns the React state it writes to.
 */
export function useWorkerTicker({ started, active, reducedMotion }: Options) {
  const [query, setQuery] = useState("")
  /** the whole query is highlighted, about to be replaced */
  const [selected, setSelected] = useState(false)
  const [cards, setCards] = useState<TickerCard[]>([])
  /** width of cards pruned off the left edge, kept as a spacer so nothing shifts */
  const [spacer, setSpacer] = useState(0)
  const [focusedKey, setFocusedKey] = useState<number | null>(null)
  const [command, setCommand] = useState<CardCommand | null>(null)

  const x: MotionValue<number> = useMotionValue(0)
  const viewportRef = useRef<HTMLDivElement>(null)
  const cardEls = useRef(new Map<number, HTMLDivElement>())
  const focusedRef = useRef<number | null>(null)
  const animatingRef = useRef(false)

  const activeRef = useRef(active)
  const resumeRef = useRef<(() => void)[]>([])
  const reducedRef = useRef(reducedMotion)
  /** the runner is waiting for the latest `cards` to reach the DOM */
  const commitListeners = useRef<(() => void)[]>([])

  useEffect(() => {
    reducedRef.current = reducedMotion
  }, [reducedMotion])

  useEffect(() => {
    activeRef.current = active
    if (active) {
      for (const resume of resumeRef.current.splice(0)) resume()
    }
  }, [active])

  const registerCard = useCallback((key: number, el: HTMLDivElement | null) => {
    if (el) cardEls.current.set(key, el)
    else cardEls.current.delete(key)
  }, [])

  /** translateX that centres the card in the viewport */
  const centreOf = useCallback((key: number) => {
    const el = cardEls.current.get(key)
    const vp = viewportRef.current
    if (!el || !vp) return null
    return -(el.offsetLeft - (vp.clientWidth - el.offsetWidth) / 2)
  }, [])

  // The new cards are in the DOM (their refs are attached before this runs),
  // so the runner can measure them and centre the target.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `cards` is the trigger, not an input; this must run on every commit of it
  useLayoutEffect(() => {
    for (const listener of commitListeners.current.splice(0)) listener()
  }, [cards])

  // Keep the focused card centred when the viewport is resized.
  useEffect(() => {
    const vp = viewportRef.current
    if (!vp) return
    const ro = new ResizeObserver(() => {
      if (animatingRef.current || focusedRef.current === null) return
      const target = centreOf(focusedRef.current)
      if (target !== null) x.set(target)
    })
    ro.observe(vp)
    return () => ro.disconnect()
  }, [centreOf, x])

  useEffect(() => {
    if (!started) return
    return startTicker({
      x,
      isActive: () => activeRef.current,
      isReducedMotion: () => reducedRef.current,
      resumeQueue: resumeRef.current,
      getViewport: () => viewportRef.current,
      cardEls: cardEls.current,
      centreOf,
      setAnimating: (animating) => {
        animatingRef.current = animating
      },
      setFocused: (key) => {
        focusedRef.current = key
        setFocusedKey(key)
      },
      setQuery,
      setSelected,
      setCards,
      setSpacer,
      setCommand,
      onCardsCommit: (listener) => {
        commitListeners.current.push(listener)
        return () => {
          const i = commitListeners.current.indexOf(listener)
          if (i !== -1) commitListeners.current.splice(i, 1)
        }
      },
    })
  }, [started, centreOf, x])

  return { query, selected, cards, spacer, focusedKey, command, x, viewportRef, registerCard }
}
