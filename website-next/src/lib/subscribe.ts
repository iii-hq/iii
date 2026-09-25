import { track } from "@/lib/analytics"

// Mailmodo form endpoint. Unset on preview/demo deployments so test signups
// never reach the production list; the form still "succeeds" locally.
const MAILMODO_FORM_URL = process.env.NEXT_PUBLIC_MAILMODO_FORM_URL ?? ""

const REQUESTED_KEY = "iii_access_requested"
const EMAIL_KEY = "iii_access_email"

/**
 * Subscribe an email to development updates. Never throws: the UI marks the
 * form submitted immediately, and delivery is best-effort (Mailmodo answers
 * 409 for an address that is already subscribed, which counts as success).
 */
export async function subscribe(email: string, location: string) {
  track("email_submit", { form_location: location })
  try {
    localStorage.setItem(REQUESTED_KEY, "true")
    localStorage.setItem(EMAIL_KEY, email)
  } catch {
    // storage unavailable: the form still shows its submitted state
  }
  if (!MAILMODO_FORM_URL) return
  try {
    const res = await fetch(MAILMODO_FORM_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email }),
    })
    if (res.ok || res.status === 409) window.iiiNotifyCommonRoomEmail?.(email)
  } catch {
    // best-effort delivery: never block the visitor on a network error
  }
}
