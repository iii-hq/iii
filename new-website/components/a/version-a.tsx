import { Faq } from '@/components/sections/faq'
import { Footer } from '@/components/site/footer'
import { SiteHeader } from '@/components/site/site-header'
import { tokenize } from '@/lib/highlight'
import { story } from './content'
import { FinalCta } from './final-cta'
import { Hero } from './hero'
import { LiveDemo } from './live-demo'
import { Overview } from './overview'
import { Ownership } from './ownership'
import { Proof } from './proof'
import { getPageStats } from './stats'
import { Story } from './story'
import { UseCases } from './use-cases'

/** Version A of the A/B landing page test: one iii graph that grows as the page scrolls. */
export async function VersionA() {
  const [stats, files, chain, extend] = await Promise.all([
    getPageStats(),
    Promise.all(story.compose.files.map((f) => tokenize(f.code.join('\n'), f.lang))),
    tokenize(story.compose.chainCode.join('\n'), 'ts'),
    tokenize(story.extend.code.join('\n'), 'ts'),
  ])
  return (
    <>
      <SiteHeader />
      <main>
        <Hero />
        <Overview />
        <LiveDemo />
        <Story code={{ files, chain, extend }} />
        <Proof />
        <UseCases />
        <Ownership stats={stats} />
        <Faq />
        <FinalCta />
      </main>
      <Footer />
    </>
  )
}
