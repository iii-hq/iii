import type { Metadata } from 'next'

import { EmailSignup } from '@/components/site/email-signup'
import { SubscribeStatus } from '@/components/site/subscribe-status'

/* Reached only from the confirmation email, so it stays out of search. */
export const metadata: Metadata = {
  title: { absolute: 'iii / confirmation link expired' },
  robots: { index: false, follow: true },
}

export default function SubscribeExpiredPage() {
  return (
    <SubscribeStatus title="This link didn't work">
      <p>
        The confirmation link has expired or couldn't be checked. Enter your email again and we'll send you a fresh one.
      </p>
      <EmailSignup location="subscribe-expired" className="max-w-md" />
    </SubscribeStatus>
  )
}
