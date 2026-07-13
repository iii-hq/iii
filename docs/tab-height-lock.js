;(() => {
  // Reserve each tab group's height so switching panels never shrinks the
  // block. A method's Parameters panel (nested field lists) is much taller
  // than its Example panel (one code block), so toggling between them would
  // otherwise collapse the block and pull the rest of the page up. We lock
  // each tab component to the tallest height it has rendered; paired with
  // `overflow-anchor: none` in custom.css, a tab change holds the tab strip
  // fixed and only ever grows the block downward.
  //
  // Mintlify includes every .js file in the content dir on each page and
  // renders Tabs with a standard ARIA `role="tablist"`; the tablist's parent
  // is the tab component (tablist + panel). No-op on pages without tabs.
  const observed = new WeakSet()

  function lock(root) {
    // minHeight only ever ratchets up to the tallest natural height seen, so
    // this is idempotent: once locked to the tall panel, switching to a short
    // one keeps the reserved height instead of collapsing.
    const prev = parseFloat(root.style.minHeight) || 0
    const natural = root.offsetHeight
    if (natural > prev) root.style.minHeight = natural + 'px'
  }

  function attach(list) {
    const root = list.parentElement
    if (!root || observed.has(root)) return
    observed.add(root)
    lock(root)
    if (typeof ResizeObserver !== 'undefined') {
      new ResizeObserver(() => lock(root)).observe(root)
    }
  }

  function scan() {
    document.querySelectorAll('[role="tablist"]').forEach(attach)
  }

  // Coalesce the frequent subtree mutations (copy buttons, tooltips, panel
  // swaps) into one scan per frame.
  let queued = false
  function scheduleScan() {
    if (queued) return
    queued = true
    requestAnimationFrame(() => {
      queued = false
      scan()
    })
  }

  // Initial render, plus client-side navigation (Mintlify is a SPA, so new
  // pages mount without a full reload).
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', scan)
  } else {
    scan()
  }
  new MutationObserver(scheduleScan).observe(document.body, { childList: true, subtree: true })
})()
