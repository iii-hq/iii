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
        This confirmation link has expired or is no longer valid. Enter your email again and we'll send you a new one.
      </p>
      <EmailSignup location="subscribe-expired" className="w-full max-w-md" />
    </SubscribeStatus>
  )
}
