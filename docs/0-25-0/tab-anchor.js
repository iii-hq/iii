;(() => {
  // Keep a tab strip where it was on screen when one of its tabs is chosen.
  //
  // Panels in a tab group differ wildly in height (a Rust example is often
  // twice the TypeScript one), and Mintlify syncs same-titled tab groups
  // page-wide: picking Rust in one group flips every language group on the
  // page. Groups ABOVE the clicked one then grow or shrink and push the
  // clicked strip up or down even though the scroll position never moved.
  //
  // So on activation we note where the strip sits in the viewport and, for
  // the next few frames, scroll by however far it drifted. The strip and its
  // tabs stay under the pointer; only content below the strip moves. Paired
  // with `overflow-anchor: none` in custom.css so the browser's own scroll
  // anchoring doesn't fight the correction.
  const SETTLE_MS = 400

  // Keys Mintlify's tab list handles itself. Arrow Up/Down, Page Up/Down and
  // the like scroll the page, so they must not start a correction.
  const TAB_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'Home', 'End', 'Enter', ' '])

  let release = null

  function anchor(tablist) {
    if (release) release()

    const startTop = tablist.getBoundingClientRect().top
    const until = performance.now() + SETTLE_MS
    let frame = 0

    let lastY = window.scrollY

    const tick = () => {
      // Navigated away: Mintlify replaced the page and this strip is gone.
      if (!tablist.isConnected) return release()

      // Our corrections leave scrollY at lastY, and with scroll anchoring off a
      // panel swap doesn't move it either. Any other change is the reader
      // scrolling (scrollbar drag, keys), unless the page got shorter and the
      // browser clamped scrollY to the new bottom.
      const y = window.scrollY
      const maxY = document.documentElement.scrollHeight - window.innerHeight
      if (Math.abs(y - lastY) > 1 && !(y < lastY && y >= maxY - 1)) return release()

      const drift = tablist.getBoundingClientRect().top - startTop
      if (drift) window.scrollBy({ top: drift, behavior: 'instant' })
      lastY = window.scrollY
      if (performance.now() < until) frame = requestAnimationFrame(tick)
      else release()
    }

    // A wheel or touch scroll means the reader has moved on; stop correcting.
    const onUserScroll = () => release && release()

    release = () => {
      cancelAnimationFrame(frame)
      window.removeEventListener('wheel', onUserScroll, true)
      window.removeEventListener('touchstart', onUserScroll, true)
      release = null
    }

    window.addEventListener('wheel', onUserScroll, { capture: true, passive: true })
    window.addEventListener('touchstart', onUserScroll, { capture: true, passive: true })
    frame = requestAnimationFrame(tick)
  }

  // Capture phase, so the starting position is recorded before Mintlify's own
  // handler swaps the panels.
  document.addEventListener(
    'click',
    event => {
      const tab = event.target && event.target.closest && event.target.closest('[role="tab"]')
      const tablist = tab && tab.closest('[role="tablist"]')
      if (tablist) anchor(tablist)
    },
    true,
  )

  document.addEventListener(
    'keydown',
    event => {
      const tablist = event.target && event.target.closest && event.target.closest('[role="tablist"]')
      if (tablist && TAB_KEYS.has(event.key)) anchor(tablist)
      else if (release) release()
    },
    true,
  )
})()
