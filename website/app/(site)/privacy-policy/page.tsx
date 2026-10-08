import type { Metadata } from 'next'

import styles from '@/components/article/article.module.css'
import { PageHead } from '@/components/article/page-head'
import { Prose } from '@/components/article/prose'
import { TocRail } from '@/components/article/toc-rail'
import { wideContainer } from '@/components/landing/section'
import { Footer } from '@/components/site/footer'
import { SiteHeader } from '@/components/site/site-header'
import { renderMarkdown } from '@/lib/markdown'
import { pageMetadata } from '@/lib/seo'
import { cn } from '@/lib/utils'
import { body, privacy } from './privacy-data'

const description =
  'Privacy policy for the iii.dev website, operated by Motia LLC. What we collect, how we use it, the analytics providers we use, and your choices.'

const base = pageMetadata({ title: 'iii / privacy policy', description, path: '/privacy-policy' })
export const metadata: Metadata = {
  ...base,
  keywords: [
    'iii',
    'privacy policy',
    'data protection',
    'website analytics',
    'cookies',
    'data security',
    'GDPR',
    'developer privacy',
  ],
  authors: [{ name: 'Motia LLC' }],
  openGraph: { ...base.openGraph, type: 'article' },
}

/**
 * The policy as one article: the page head, the sections in `<Prose>`, and on wide screens a sticky contents list in
 * the trailing column.
 */
export default async function PrivacyPolicyPage() {
  const { content, headings } = await renderMarkdown(body)
  const sections = headings.filter((h) => h.depth === 2)

  return (
    <>
      <SiteHeader />
      <main>
        <div className={cn(wideContainer, styles.page, styles.layout)}>
          <article aria-labelledby="privacy-title" className={styles.main}>
            <PageHead
              eyebrow={privacy.eyebrow}
              title={privacy.title}
              titleId="privacy-title"
              description={privacy.lede}
            >
              <p className={styles.meta}>Last updated {privacy.updated}</p>
            </PageHead>

            <Prose className="mt-12 sm:mt-16 [&_.doc-updated]:mt-6 [&_.doc-updated]:text-[13px]">{content}</Prose>
          </article>

          <aside className={styles.aside}>
            <TocRail groups={[{ items: sections }]} />
          </aside>
        </div>
      </main>
      <Footer container={wideContainer} />
    </>
  )
}
