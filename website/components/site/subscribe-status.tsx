import type { ReactNode } from 'react'

import { wideContainer } from '@/components/landing/section'
import { Footer } from '@/components/site/footer'
import { PixelHeading } from '@/components/site/pixel-heading'
import { SiteHeader } from '@/components/site/site-header'
import { cn } from '@/lib/utils'

/**
 * Where the confirmation link of the updates signup lands (services/subscribe redirects to /subscribed or
 * /subscribe-expired): a heading, a short message, and whatever comes next.
 */
export function SubscribeStatus({ title, children }: { title: string; children: ReactNode }) {
  return (
    <>
      <SiteHeader />
      <main className={cn(wideContainer, 'py-40')}>
        <div className="max-w-xl">
          <PixelHeading as="h1" className="text-[40px]">
            {title}
          </PixelHeading>
          <div className="mt-6 space-y-6 text-pretty text-[15px] text-muted-foreground leading-relaxed">{children}</div>
        </div>
      </main>
      <Footer container={wideContainer} />
    </>
  )
}
