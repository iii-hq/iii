import { track } from '@/lib/analytics'

// Signup endpoint: CloudFront proxies /api/subscribe to the signup API on iii
// Cloud (services/subscribe). It emails a confirmation link to /subscribe-confirm,
// whose button posts to /api/subscribe/confirm; only then does the address join
// Resend's "iii dev Website Signups" segment.
// Production builds post to it unless NEXT_PUBLIC_SUBSCRIBE_URL overrides it; set
// the variable to an empty string on preview/demo projects so test signups never
// reach the list. `next dev` sends nothing unless it is set.
const SUBSCRIBE_URL =
  process.env.NEXT_PUBLIC_SUBSCRIBE_URL ?? (process.env.NODE_ENV === 'production' ? '/api/subscribe' : '')

const REQUESTED_KEY = 'iii_access_requested'
const EMAIL_KEY = 'iii_access_email'

/**
 * Subscribe an email to development updates. Never throws: the UI marks the
 * form submitted immediately, and delivery is best-effort (an address that is
 * already subscribed also answers 200, and gets no email).
 */
export async function subscribe(email: string, location: string) {
  track('email_submit', { form_location: location })
  try {
    localStorage.setItem(REQUESTED_KEY, 'true')
    localStorage.setItem(EMAIL_KEY, email)
  } catch {
    // storage unavailable: the form still shows its submitted state
  }
  if (!SUBSCRIBE_URL) return
  try {
    const res = await fetch(SUBSCRIBE_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, source: 'website' }),
    })
    if (res.ok) window.iiiNotifyCommonRoomEmail?.(email)
  } catch {
    // best-effort delivery: never block the visitor on a network error
  }
}

export type ConfirmResult = 'confirmed' | 'invalid' | 'failed'

/**
 * Confirm a subscription with the token from the emailed link. `invalid` is a
 * bad or expired link (a new one is needed); `failed` is anything else, worth
 * trying again.
 */
export async function confirmSubscription(token: string): Promise<ConfirmResult> {
  if (!SUBSCRIBE_URL) return 'failed'
  try {
    const res = await fetch(`${SUBSCRIBE_URL}/confirm`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    })
    if (res.ok) return 'confirmed'
    return res.status === 400 ? 'invalid' : 'failed'
  } catch {
    return 'failed'
  }
}
