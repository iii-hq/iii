import { animate, type MotionValue } from "motion/react"
import { addCommand, buildPlayOrder, nextPlayOrder, randomWorkersExcept, registersLine, type Worker } from "./catalog"

// Timings (ms). This is an explanatory marketing animation, so it may run
// slower than UI motion, and it should: the reader is watching, not waiting.
/** how long the focused card holds before the next search starts */
const FOCUS_DWELL = 6000
/** the strip glides a few cards; already on screen, so it eases in and out */
const TRANSITION = 1900
/** the query sits finished before anything moves, so it can be read */
const PRE_SCROLL_PAUSE = 700
/** the query is shown selected (⌘A) for this long before it is replaced */
const SELECT_HOLD = 420
/** cards between the current one and the next match; sets the glide distance */
const FILLER_COUNT = 2
/** ease-in-out cubic: accelerates, cruises, brakes, never lurches */
const EASE = [0.65, 0, 0.35, 1] as const
/** per-character typing, ms: a person at a search box vs. a terminal */
const SEARCH_KEY = { base: 95, jitter: 70 }
const COMMAND_KEY = { base: 30, jitter: 18 }

type KeyTiming = { base: number; jitter: number }

export type TickerCard = { key: number; worker: Worker }

/** The `$` / `›` lines being typed into the focused card. */
export type CardCommand = { key: number; add: string; trigger: string }

/** a React state setter, accepting a value or an updater */
type Setter<T> = (next: T | ((prev: T) => T)) => void

export type TickerDeps = {
  /** translateX of the card strip */
  x: MotionValue<number>
  /** the ticker is on screen; the sequence pauses between steps when false */
  isActive: () => boolean
  isReducedMotion: () => boolean
  /** callbacks to run once the ticker is back on screen; the owner drains it */
  resumeQueue: (() => void)[]
  getViewport: () => HTMLDivElement | null
  /** mounted card elements by card key */
  cardEls: ReadonlyMap<number, HTMLDivElement>
  /** translateX that centres the card in the viewport */
  centreOf: (key: number) => number | null
  /** the strip is mid-glide; the owner should not re-centre it */
  setAnimating: (animating: boolean) => void
  setQuery: (query: string) => void
  setSelected: (selected: boolean) => void
  setCards: Setter<TickerCard[]>
  setSpacer: Setter<number>
  setFocused: (key: number | null) => void
  setCommand: (command: CardCommand | null) => void
  /**
   * Subscribe to the next commit of `cards`, once the new card elements are in
   * the DOM and measurable. Returns an unsubscribe.
   */
  onCardsCommit: (listener: () => void) => () => void
}

class Cancelled extends Error {}

/**
 * Runs the showcase sequence: types a search term into the registry search
 * field, then glides the strip of worker cards until the matching worker sits
 * centred and highlighted, and types its install command. Repeats until the
 * returned stop function is called, which cancels every pending step and
 * resets the state it drove.
 *
 * Kept outside React so the hook's effect stays synchronous; the runner only
 * ever talks to React through the setters in `deps`.
 */
export function startTicker(deps: TickerDeps): () => void {
  const {
    x,
    isActive,
    isReducedMotion,
    resumeQueue,
    getViewport,
    cardEls,
    centreOf,
    setAnimating,
    setQuery,
    setSelected,
    setCards,
    setSpacer,
    setFocused,
    setCommand,
    onCardsCommit,
  } = deps

  let cancelled = false
  let nextKey = 0
  let cmdToken = 0
  let currentQuery = ""
  const timers = new Set<ReturnType<typeof setTimeout>>()
  /** abandons a pending `cardsCommitted`; called on stop */
  const commitWaiters = new Set<() => void>()
  let scroll: ReturnType<typeof animate> | null = null

  /** call `done` once the ticker is on screen, `fail` once it has stopped */
  const whenActive = (done: () => void, fail: (e: Error) => void) => {
    if (cancelled) fail(new Cancelled())
    else if (isActive()) done()
    else resumeQueue.push(() => whenActive(done, fail))
  }

  /** sleep, then hold while the ticker is off screen; fails once stopped */
  const after = (ms: number, done: () => void, fail: (e: Error) => void) => {
    const t = setTimeout(() => {
      timers.delete(t)
      whenActive(done, fail)
    }, ms)
    timers.add(t)
  }

  const wait = (ms: number) => new Promise<void>((resolve, reject) => after(ms, resolve, reject))

  /**
   * Reveal `text` one character at a time with a person's uneven key timing.
   * Each key schedules the next, so a pause or stop lands between keystrokes.
   * Resolves early once `stale()` reports the text is no longer wanted.
   */
  const typeText = (
    text: string,
    key: KeyTiming,
    onChar: (typed: string) => void,
    stale: () => boolean = () => false,
  ) =>
    new Promise<void>((resolve, reject) => {
      const step = (i: number) => {
        if (i > text.length || stale()) {
          resolve()
          return
        }
        onChar(text.slice(0, i))
        after(key.base + Math.random() * key.jitter, () => step(i + 1), reject)
      }
      step(1)
    })

  const write = (text: string) => {
    currentQuery = text
    setQuery(text)
  }

  const showQueryFor = async (w: Worker) => {
    const label = w.query
    if (isReducedMotion()) {
      write(label)
      return
    }
    // Select-all, then type over it: one calm replacement instead of a
    // flicker of backspaces.
    if (currentQuery) {
      setSelected(true)
      await wait(SELECT_HOLD)
      setSelected(false)
      write("")
      await wait(180)
    }
    await typeText(label, SEARCH_KEY, write)
    await wait(260)
  }

  const typeCommands = async (key: number, w: Worker) => {
    const token = ++cmdToken
    /** a newer card has started typing; this one stops where it is */
    const stale = () => token !== cmdToken
    const add = addCommand(w)
    const trigger = registersLine(w)
    setCommand({ key, add: "", trigger: "" })
    if (isReducedMotion()) {
      setCommand({ key, add, trigger })
      return
    }
    try {
      await wait(600)
      await typeText(add, COMMAND_KEY, (typed) => setCommand({ key, add: typed, trigger: "" }), stale)
      if (stale()) return
      await wait(650)
      await typeText(trigger, COMMAND_KEY, (typed) => setCommand({ key, add, trigger: typed }), stale)
    } catch (e) {
      if (!(e instanceof Cancelled)) throw e
    }
  }

  /** resolves once React has committed the latest `setCards`; fails once stopped */
  const cardsCommitted = () =>
    new Promise<void>((resolve, reject) => {
      const off = onCardsCommit(() => {
        commitWaiters.delete(abandon)
        if (cancelled) reject(new Cancelled())
        else resolve()
      })
      const abandon = () => {
        off()
        reject(new Cancelled())
      }
      commitWaiters.add(abandon)
    })

  /** add cards to the strip; resolves once they can be measured */
  const append = async (workers: Worker[]) => {
    const added = workers.map((worker) => ({ key: nextKey++, worker }))
    setCards((prev) => [...prev, ...added])
    await cardsCommitted()
    return added
  }

  /** drop cards far off the left edge; their width moves into the spacer */
  const prune = () => {
    const vp = getViewport()
    if (!vp) return
    const hideBefore = -x.get() - vp.clientWidth * 1.5
    const gone = new Set<number>()
    let width = 0
    for (const [key, el] of cardEls) {
      if (el.offsetLeft + el.offsetWidth < hideBefore) {
        gone.add(key)
        width += el.offsetWidth
      }
    }
    if (!gone.size) return
    // Both land in one commit, so the strip never shifts.
    setCards((prev) => prev.filter((c) => !gone.has(c.key)))
    setSpacer((s) => s + width)
  }

  const focusWorker = async (w: Worker) => {
    await showQueryFor(w)
    await wait(PRE_SCROLL_PAUSE)
    const added = await append([...randomWorkersExcept(w, FILLER_COUNT), w, ...randomWorkersExcept(w, 1)])
    const targetKey = added[FILLER_COUNT].key
    const targetX = centreOf(targetKey)
    setFocused(null)
    if (targetX !== null) {
      if (isReducedMotion()) x.set(targetX)
      else {
        setAnimating(true)
        scroll = animate(x, targetX, { duration: TRANSITION / 1000, ease: EASE })
        scroll.then(() => setAnimating(false))
      }
    }
    await wait(TRANSITION)
    setFocused(targetKey)
    prune()
    void typeCommands(targetKey, w)
    await wait(FOCUS_DWELL)
  }

  const run = async () => {
    await wait(0)
    let order = buildPlayOrder()
    const first = order[0]
    const seeded = await append([...randomWorkersExcept(first, 1), first, ...randomWorkersExcept(first, 2)])
    const firstX = centreOf(seeded[1].key)
    if (firstX !== null) x.set(firstX)
    setFocused(seeded[1].key)
    await showQueryFor(first)
    void typeCommands(seeded[1].key, first)
    await wait(FOCUS_DWELL)

    for (let i = 1; ; i++) {
      if (i >= order.length) {
        order = nextPlayOrder(order)
        i = 0
      }
      await focusWorker(order[i])
    }
  }

  run().catch((e) => {
    if (!(e instanceof Cancelled)) throw e
  })

  return () => {
    cancelled = true
    for (const t of timers) clearTimeout(t)
    timers.clear()
    for (const resume of resumeQueue.splice(0)) resume()
    for (const abandon of [...commitWaiters]) abandon()
    commitWaiters.clear()
    scroll?.stop()
    setAnimating(false)
    setFocused(null)
    x.set(0)
    setCards([])
    setSpacer(0)
    setCommand(null)
    setQuery("")
    setSelected(false)
  }
}
