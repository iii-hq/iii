import { Faq } from '@/components/sections/faq'
import { Footer } from '@/components/site/footer'
import { SiteHeader } from '@/components/site/site-header'
import { links } from '@/lib/site'
import { FinalCta } from './final-cta'
import { Hero } from './hero'
import { LiveDemo } from './live-demo'
import { Numbers } from './numbers'
import { Overview } from './overview'
import { Ownership } from './ownership'
import { Proof } from './proof'
import { wideContainer } from './section'
import { getPageStats } from './stats'
import { Story } from './story'
import { UseCases } from './use-cases'

const faqLink =
  'text-foreground underline decoration-line-strong underline-offset-4 transition-colors hover:decoration-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 rounded-sm'

/** The iii.dev landing page: one iii graph that grows as the page scrolls. */
export async function Landing() {
  const stats = await getPageStats()
  return (
    <>
      <SiteHeader />
      <main>
        <Hero />
        <Overview />
        {/* Ownership sits before the Live demo: the 2026-10-05 sync moved it up as a top-level value prop. */}
        <Ownership stats={stats} />
        <LiveDemo />
        <Story />
        <Proof />
        <UseCases />
        {/* Numbers keeps the doc's slot after Use cases: the engine benchmarks, with the runner in a footnote. */}
        <Numbers />
        <Faq
          container={wideContainer}
          lede={
            <>
              Something else? Ask in{' '}
              <a href={links.discord} className={faqLink}>
                Discord
              </a>{' '}
              or open an issue on{' '}
              <a href={links.github} className={faqLink}>
                GitHub
              </a>
              .
            </>
          }
        />
        <FinalCta />
      </main>
      <Footer container={wideContainer} />
    </>
  )
}
