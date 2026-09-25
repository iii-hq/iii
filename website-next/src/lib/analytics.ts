// Analytics are consent-gated AND opt-in per deployment: nothing loads unless
// NEXT_PUBLIC_ENABLE_ANALYTICS=true, so preview/demo deployments never send
// events into the production GTM / PostHog / Common Room properties.
export const ANALYTICS_ENABLED = process.env.NEXT_PUBLIC_ENABLE_ANALYTICS === "true"

/** localStorage key shared with the Astro site (cross-page consent contract). */
export const CONSENT_KEY = "iii_cookie_consent"

export type Consent = "accepted" | "rejected"

type Params = Record<string, string | number | boolean | undefined>

declare global {
  interface Window {
    iiiTrack?: (event: string, params?: Params) => void
    iiiLoadGTM?: () => void
    iiiLoadCommonRoomSignals?: () => void
    iiiLoadPostHog?: () => void
    iiiNotifyCommonRoomEmail?: (email: string) => void
  }
}

export function readConsent(): Consent | null {
  try {
    const v = localStorage.getItem(CONSENT_KEY)
    return v === "accepted" || v === "rejected" ? v : null
  } catch {
    return null
  }
}

export function writeConsent(value: Consent) {
  try {
    localStorage.setItem(CONSENT_KEY, value)
  } catch {
    // storage unavailable: consent applies for this page view only
  }
  if (value === "accepted") {
    window.iiiLoadGTM?.()
    window.iiiLoadCommonRoomSignals?.()
    window.iiiLoadPostHog?.()
  }
}

/** Push a named event to the GTM dataLayer. Safe no-op without consent. */
export function track(event: string, params?: Params) {
  if (typeof window === "undefined") return
  window.iiiTrack?.(event, params)
}

/** Shorthand for the site-wide `cta_click` event. */
export function trackCta(id: string, location: string, extra?: Params) {
  track("cta_click", { cta_id: id, cta_location: location, ...extra })
}
