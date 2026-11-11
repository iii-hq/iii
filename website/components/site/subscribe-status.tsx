import type { ReactNode } from 'react'

import { wideContainer } from '@/components/landing/section'
import { Footer } from '@/components/site/footer'
import { PixelHeading } from '@/components/site/pixel-heading'
import { SiteHeader } from '@/components/site/site-header'
import { cn } from '@/lib/utils'

/**
 * The pages of the updates signup's confirmation (/subscribe-confirm, where the emailed link lands, then /subscribed or
 * /subscribe-expired): a heading, a short message, and whatever comes next.
 */
export function SubscribeStatus({ title, children }: { title: string; children: ReactNode }) {
  return (
    <>
      <SiteHeader />
      {/* At least a full screen, so the footer starts below the fold; the message sits in its middle (the header is
          fixed over it). */}
      <main className={cn(wideContainer, 'flex min-h-svh items-center justify-center py-32')}>
        <div className="flex w-full max-w-xl flex-col items-center text-center">
          <PixelHeading as="h1" className="text-[40px]">
            {title}
          </PixelHeading>
          <div className="mt-6 flex flex-col items-center space-y-6 text-balance text-[15px] text-muted-foreground leading-relaxed">
            {children}
          </div>
        </div>
      </main>
      <Footer container={wideContainer} />
    </>
  )
}
