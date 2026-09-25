"use client"

import { useInView } from "motion/react"
import { type MouseEvent, useCallback, useEffect, useRef, useState } from "react"
import { Swap } from "@/components/motion/swap"
import { PauseIcon, PlayIcon, RefreshIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { useTheme } from "@/hooks/use-theme"
import { cn } from "@/lib/utils"
import { adoptSiteFonts } from "./adopt-site-fonts"

const DEMO_SRC = "/console-demo/index.html"

// The demo lays itself out for a 1440px desktop canvas (three columns) or a
// ~400px phone canvas (its "simplified view"). Anything in between clips a
// column, so the iframe is always rendered at one of those two widths and
// scaled to fit the frame.
const DESKTOP_CANVAS = 1440
const PHONE_CANVAS = 400
const PHONE_BREAKPOINT = 768
/** Height of the demo's own title bar, in canvas px. We draw our own, so it's clipped away. */
const DEMO_CHROME = 45

const currentTheme = () => (document.documentElement.classList.contains("dark") ? "dark" : "light")

/**
 * Whether `el` can still scroll by `dy`. Slack rather than exactly-at-the-end:
 * a streaming transcript keeps growing, so a pane the reader just bottomed out
 * has room again a moment later and would take the wheel back over and over.
 */
function canScroll(el: Element, dy: number) {
  const overflowY = el.ownerDocument.defaultView?.getComputedStyle(el).overflowY
  if (overflowY !== "auto" && overflowY !== "scroll") return false
  const room = el.scrollHeight - el.clientHeight
  const slack = el.clientHeight * 0.15
  return room > slack && (dy > 0 ? el.scrollTop < room - slack : el.scrollTop > slack)
}

const chromeButton = cn(buttonVariants({ variant: "ghost", size: "icon-xs" }), "text-gray-10 hover:text-gray-12")

/**
 * The vendored console demo inside a macOS-style window: bezel, title bar with
 * traffic lights, our own Pause / Replay controls (the demo's bar is clipped
 * away). Lazy-loads a viewport ahead, plays only while mostly on screen,
 * follows the site theme, and hands the wheel back to the page once the demo's
 * panes have nothing left to scroll. A transparent cover keeps the first click
 * and the wheel with the page until the reader claims the console.
 */
export function ConsoleFrame({ className }: { className?: string }) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const [claimed, setClaimed] = useState(false)
  const [paused, setPaused] = useState(false)

  // Measure the viewport; the canvas scale follows from its width.
  const [box, setBox] = useState({ width: 0, height: 0 })
  useEffect(() => {
    const el = viewportRef.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect
      setBox({ width, height })
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const canvas = box.width < PHONE_BREAKPOINT ? PHONE_CANVAS : DESKTOP_CANVAS
  const scale = box.width ? box.width / canvas : 1

  // Load a viewport ahead, mounted paused, so it's ready when it arrives.
  const near = useInView(viewportRef, { margin: "100% 0px", once: true })
  const [src, setSrc] = useState<string>()
  useEffect(() => {
    if (near) setSrc((s) => s ?? `${DEMO_SRC}?theme=${currentTheme()}&paused=1`)
  }, [near])

  // Play while at least 60% is visible and not paused by the reader.
  const inView = useInView(viewportRef, { amount: 0.6 })
  const visible = useInView(viewportRef)
  const active = inView && !paused
  const post = useCallback((message: Record<string, unknown>) => {
    iframeRef.current?.contentWindow?.postMessage(message, "*")
  }, [])
  useEffect(() => {
    post({ type: "iii-demo", active })
  }, [active, post])
  // Scrolling the console off screen hands the wheel and first click back to the page.
  useEffect(() => {
    if (!visible) setClaimed(false)
  }, [visible])

  // The demo reads the theme once at load; carry later flips into the frame.
  const { theme } = useTheme()
  useEffect(() => {
    post({ type: "iii-demo-theme", theme })
  }, [theme, post])

  // A wheel over a pane that can't scroll any further would otherwise go
  // nowhere. Forward it to the page 1:1 so the reader is never stuck.
  const onWheel = useCallback((e: WheelEvent) => {
    const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaMode === 2 ? e.deltaY * window.innerHeight : e.deltaY
    if (!dy) return
    for (let el = e.target as Element | null; el; el = el.parentElement) if (canScroll(el, dy)) return
    window.scrollBy({ top: dy, behavior: "instant" })
  }, [])

  // The demo's document once loaded; the wheel listener follows it and is removed with it.
  const [demoDoc, setDemoDoc] = useState<Document | null>(null)
  useEffect(() => {
    if (!demoDoc) return
    demoDoc.addEventListener("wheel", onWheel, { passive: true })
    return () => demoDoc.removeEventListener("wheel", onWheel)
  }, [demoDoc, onWheel])

  const onLoad = () => {
    post({ type: "iii-demo", active })
    const doc = iframeRef.current?.contentDocument
    if (!doc) return
    setDemoDoc(doc)
    try {
      adoptSiteFonts(doc)
    } catch {
      /* cross-origin dev setups: the demo keeps its own type */
    }
  }

  // The claim click is forwarded into the demo at the same point, so the first
  // click acts on what the reader aimed at instead of only dissolving the cover.
  const onClaim = (e: MouseEvent<HTMLButtonElement>) => {
    setClaimed(true)
    const iframe = iframeRef.current
    if (!iframe) return
    try {
      const r = iframe.getBoundingClientRect()
      const x = (e.clientX - r.left) * (iframe.clientWidth / r.width)
      const y = (e.clientY - r.top) * (iframe.clientHeight / r.height)
      const target = iframe.contentDocument?.elementFromPoint(x, y) as HTMLElement | null | undefined
      if (typeof target?.click === "function") target.click()
    } catch {
      /* cross-origin dev setups: the claim still works, only unforwarded */
    }
    iframe.contentWindow?.focus()
  }

  // Escape, or the demo's own close message, hands control back to the page.
  useEffect(() => {
    const release = () => {
      setClaimed(false)
      ;(document.activeElement as HTMLElement | null)?.blur()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") release()
    }
    const onMessage = (e: MessageEvent) => {
      if (e.source === iframeRef.current?.contentWindow && e.data?.type === "iii-demo-close") release()
    }
    document.addEventListener("keydown", onKey)
    window.addEventListener("message", onMessage)
    return () => {
      document.removeEventListener("keydown", onKey)
      window.removeEventListener("message", onMessage)
    }
  }, [])

  function replay() {
    setPaused(false)
    post({ type: "iii-demo-replay" })
  }

  return (
    // Bezel: a soft outer surface, like a window sitting on the desktop.
    <div className={cn("rounded-[22px] bg-gray-3/70 p-1.5 shadow-panel", className)}>
      <div className="overflow-hidden rounded-[16px] bg-gray-2 shadow-[inset_0_0_0_1px_var(--gray-6),inset_0_1px_0_var(--gray-7)]">
        {/* Title bar */}
        <div className="relative flex h-11 items-center justify-between border-b border-gray-5 px-4">
          <div aria-hidden="true" className="flex gap-2">
            <span className="size-3 rounded-full bg-gray-6 shadow-[inset_0_0_0_1px_var(--gray-7)]" />
            <span className="size-3 rounded-full bg-gray-6 shadow-[inset_0_0_0_1px_var(--gray-7)]" />
            <span className="size-3 rounded-full bg-gray-6 shadow-[inset_0_0_0_1px_var(--gray-7)]" />
          </div>

          <p className="pointer-events-none absolute inset-x-0 text-center text-[13px] text-gray-10 select-none">
            <span className="font-medium text-gray-12">iii console</span>
            <span className="mx-1.5">·</span>
            payments ledger
          </p>

          <div className="relative flex items-center gap-1">
            <span className="mr-2 hidden items-center gap-1.5 text-[12px] text-gray-10 sm:inline-flex">
              <span className="relative flex size-1.5">
                <span
                  aria-hidden="true"
                  className={cn(
                    "absolute inset-0 rounded-full bg-gray-9 motion-reduce:hidden",
                    active && "animate-ping [animation-duration:2s]",
                  )}
                />
                <span className="relative size-1.5 rounded-full bg-gray-9" />
              </span>
              Recorded session
            </span>
            <button
              type="button"
              aria-label={paused ? "Play the recording" : "Pause the recording"}
              aria-pressed={paused}
              onClick={() => setPaused((p) => !p)}
              className={chromeButton}
            >
              <Swap id={paused ? "play" : "pause"}>
                {paused ? <PlayIcon className="size-4" /> : <PauseIcon className="size-3.5" />}
              </Swap>
            </button>
            <button
              type="button"
              aria-label="Replay the recording"
              onClick={replay}
              className={cn(chromeButton, "group/replay")}
            >
              <RefreshIcon className="size-3.5 transition-transform duration-300 ease-out group-active/replay:-rotate-180" />
            </button>
          </div>
        </div>

        {/* Viewport */}
        <div
          ref={viewportRef}
          className="group/console relative aspect-[16/10] w-full overflow-hidden bg-gray-1 max-md:aspect-[3/4]"
        >
          <iframe
            ref={iframeRef}
            title="iii console replaying a payments ledger session"
            loading="lazy"
            src={src}
            onLoad={onLoad}
            // Same-origin demo we script into (fonts, wheel, messages); the sandbox still keeps it from steering the page.
            sandbox="allow-scripts allow-same-origin allow-popups allow-forms"
            style={{
              width: canvas,
              height: box.height ? box.height / scale + DEMO_CHROME : "100%",
              transform: `translateY(${-DEMO_CHROME * scale}px) scale(${scale})`,
              transformOrigin: "top left",
            }}
            className="block border-0"
          />

          {!claimed && (
            <button
              type="button"
              aria-label="Interact with the console"
              onClick={onClaim}
              className="absolute inset-0 cursor-pointer border-0 bg-transparent outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-gray-8"
            >
              <span
                aria-hidden="true"
                className="pointer-events-none absolute bottom-4 left-1/2 -translate-x-1/2 scale-95 rounded-full bg-gray-12 px-3 py-1.5 text-[12px] font-medium text-gray-1 opacity-0 shadow-panel transition-[opacity,scale] duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] group-hover/console:scale-100 group-hover/console:opacity-100 motion-reduce:transition-none [@media(hover:none)]:scale-100 [@media(hover:none)]:opacity-100"
              >
                Click to interact
              </span>
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
