;(() => {
  const CACHE_KEY = 'iii-navbar-counters'
  const CACHE_TTL_MS = 1000 * 60 * 10
  const DISCORD_INVITE = 'motia'

  function compact(value) {
    if (!Number.isFinite(value)) return null
    return new Intl.NumberFormat('en', {
      notation: 'compact',
      maximumFractionDigits: value >= 10000 ? 0 : 1,
    }).format(value)
  }

  function readCache() {
    try {
      const cached = JSON.parse(localStorage.getItem(CACHE_KEY) || 'null')
      if (!cached || Date.now() - cached.updatedAt > CACHE_TTL_MS) return null
      return cached.values
    } catch (_) {
      return null
    }
  }

  function writeCache(values) {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify({ updatedAt: Date.now(), values }))
    } catch (_) {
      // Ignore storage errors; counters are decorative.
    }
  }

  async function fetchCounts() {
    const cached = readCache()
    if (cached) return cached

    const discord = await fetch(`https://discord.com/api/v10/invites/${DISCORD_INVITE}?with_counts=true`).then(
      (response) => (response.ok ? response.json() : null),
    )

    const values = {
      discord: compact(discord?.approximate_member_count),
    }

    writeCache(values)
    return values
  }

  function upsertCount(link, value) {
    if (!link || !value) return

    let count = link.querySelector('.iii-live-count')
    if (!count) {
      count = document.createElement('span')
      count.className = 'iii-live-count'
      count.setAttribute('aria-hidden', 'true')
      link.appendChild(count)
    }
    count.textContent = value
    link.setAttribute('aria-label', `${link.textContent.trim()} members`)
  }

  function applyCounts(values) {
    const discordLink = document.querySelector('a[href="https://discord.gg/motia"]')

    upsertCount(discordLink, values.discord)

    if (discordLink && values.discord) {
      discordLink.setAttribute('aria-label', `Discord, ${values.discord} members`)
    }
  }

  let pending = false

  function scheduleApply(values) {
    if (pending) return
    pending = true
    requestAnimationFrame(() => {
      pending = false
      applyCounts(values)
    })
  }

  function init() {
    fetchCounts()
      .then((values) => {
        scheduleApply(values)
        new MutationObserver(() => scheduleApply(values)).observe(document.body, { childList: true, subtree: true })
      })
      .catch(() => {
        // Keep the static navbar labels if either network request is blocked.
      })
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init, { once: true })
  } else {
    init()
  }
})()
