import { type ReactNode, useCallback, useEffect, useRef, useState } from "react"

/**
 * a fill-height scroll region that fades its clipped edges and hints "scroll ↓"
 * while more content sits below the fold. remount it (via `key`) to start a
 * new content set back at the top — the datasheets key it by the selected node.
 */
export function ScrollFadePanel({ children }: { children: ReactNode }) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [canScrollUp, setCanScrollUp] = useState(false)
  const [canScrollDown, setCanScrollDown] = useState(false)

  const updateScrollState = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const { scrollTop, scrollHeight, clientHeight } = el
    const overflow = scrollHeight - clientHeight > 8
    setCanScrollUp(overflow && scrollTop > 4)
    setCanScrollDown(overflow && scrollTop + clientHeight < scrollHeight - 4)
  }, [])

  useEffect(() => {
    const scrollEl = scrollRef.current
    const contentEl = contentRef.current
    if (!scrollEl || !contentEl) return

    scrollEl.scrollTop = 0

    updateScrollState()
    const frame = requestAnimationFrame(updateScrollState)

    scrollEl.addEventListener("scroll", updateScrollState, { passive: true })
    const observer = new ResizeObserver(updateScrollState)
    observer.observe(scrollEl)
    observer.observe(contentEl)

    return () => {
      cancelAnimationFrame(frame)
      scrollEl.removeEventListener("scroll", updateScrollState)
      observer.disconnect()
    }
  }, [updateScrollState])

  return (
    <div className="relative min-h-0 flex-1 overflow-hidden">
      <div ref={scrollRef} className="h-full overflow-y-auto overscroll-contain [scrollbar-gutter:stable]">
        <div ref={contentRef}>{children}</div>
      </div>
      {canScrollUp ? (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-0 z-10 h-8 border-b border-rule bg-gradient-to-b from-bg via-bg/95 to-transparent"
        />
      ) : null}
      {canScrollDown ? (
        <>
          <div
            aria-hidden
            className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-12 bg-gradient-to-t from-bg via-bg/95 to-transparent"
          />
          <div
            aria-hidden
            className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex items-end justify-center gap-x-1.5 pb-2"
          >
            <span className="font-mono text-[9px] uppercase tracking-[0.14em] text-accent">scroll</span>
            <span className="font-mono text-[10px] leading-none text-accent">↓</span>
          </div>
        </>
      ) : null}
    </div>
  )
}
