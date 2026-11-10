import type { Metadata } from 'next'

import { SubscribeStatus } from '@/components/site/subscribe-status'
import { links } from '@/lib/site'

/* Reached only from the confirmation email, so it stays out of search. */
export const metadata: Metadata = {
  title: { absolute: 'iii / subscribed' },
  robots: { index: false, follow: true },
}

const link = 'text-foreground underline underline-offset-4'

export default function SubscribedPage() {
  return (
    <SubscribeStatus title="You're subscribed">
      <p>
        Thanks for confirming. Your welcome email is on its way. We'll write when iii ships something new: releases,
        workers, and posts. No more than that.
      </p>
      <p>
        In the meantime, start with the{' '}
        <a href={links.quickstart} className={link}>
          quickstart
        </a>{' '}
        or say hi on{' '}
        <a href={links.discord} className={link}>
          Discord
        </a>
        .
      </p>
    </SubscribeStatus>
  )
}
