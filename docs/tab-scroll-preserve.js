;(() => {
  // Switching a Mintlify tab moves focus to the newly-selected tab trigger and
  // its panel; the browser then scrolls that element into view, which yanks the
  // whole page. We want the opposite: the content below the tab may reflow to
  // the new panel's height, but the viewport scroll position should stay put.
  //
  // So we snapshot the scroll offsets the instant a tab is activated (pointer or
  // keyboard) and restore them across the next couple of frames, cancelling the
  // auto-scroll while leaving the natural reflow untouched. Mintlify renders
  // tabs with standard ARIA roles; this is a no-op anywhere without them.
  function scrollers() {
    const els = []
    const root = document.scrollingElement || document.documentElement
    if (root) els.push(root)
    const csl = document.getElementById('content-side-layout')
    if (csl && csl !== root) els.push(csl)
    return els
  }

  function preserve() {
    const snapshot = scrollers().map(el => [el, el.scrollTop])
    const restore = () => {
      for (const [el, top] of snapshot) {
        if (el.scrollTop !== top) el.scrollTop = top
      }
    }
    // The focus-driven scroll can land synchronously or a frame later, so
    // restore immediately after paint and once more on the following frame.
    requestAnimationFrame(() => {
      restore()
      requestAnimationFrame(restore)
    })
  }

  document.addEventListener(
    'pointerdown',
    event => {
      const el = event.target
      if (el && el.closest && el.closest('[role="tab"]')) preserve()
    },
    true,
  )

  document.addEventListener(
    'keydown',
    event => {
      const el = event.target
      if (el && el.closest && el.closest('[role="tablist"]')) preserve()
    },
    true,
  )
})()
