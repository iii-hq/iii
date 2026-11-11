import type { Metadata } from 'next'

import { ConfirmSubscription } from '@/components/site/confirm-subscription'
import { SubscribeStatus } from '@/components/site/subscribe-status'

/* Reached only from the confirmation email, so it stays out of search. */
export const metadata: Metadata = {
  title: { absolute: 'iii / confirm your subscription' },
  robots: { index: false, follow: true },
}

export default function SubscribeConfirmPage() {
  return (
    <SubscribeStatus title="Confirm your subscription">
      <p>One last step: confirm your email address to start getting iii updates.</p>
      <ConfirmSubscription />
    </SubscribeStatus>
  )
}
