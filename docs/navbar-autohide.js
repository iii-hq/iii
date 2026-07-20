;(() => {
  // Slide the navbar's top row away on scroll down and bring it back on
  // scroll up, GitHub-style: the tab row stays pinned. Mintlify has no
  // built-in setting for this; it exposes #navbar / #sidebar /
  // #content-side-layout for custom styling and includes every .js file in
  // the content dir on each page. The geometry lives in custom.css keyed off
  // the html-level class this script toggles.
  const MIN_DELTA = 8 // px; ignore micro-scrolls and rubber-banding jitter
  const ALWAYS_SHOW_ABOVE = 80 // px from top where the navbar never hides

  let lastY = window.scrollY
  let hidden = false

  function setHidden(next) {
    if (hidden === next) return
    hidden = next
    document.documentElement.classList.toggle('iii-navbar-hidden', next)
  }

  window.addEventListener(
    'scroll',
    () => {
      const y = window.scrollY
      const delta = y - lastY
      if (Math.abs(delta) < MIN_DELTA) return
      if (y < ALWAYS_SHOW_ABOVE) {
        setHidden(false)
      } else {
        setHidden(delta > 0)
      }
      lastY = y
    },
    { passive: true },
  )

  // Keyboard-driven focus into the navbar (e.g. tabbing to search) must
  // reveal it; a hidden bar that owns focus is unusable.
  document.addEventListener('focusin', event => {
    const el = document.getElementById('navbar')
    if (el && el.contains(event.target)) setHidden(false)
  })
})()
