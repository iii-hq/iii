import type { Metadata } from 'next'

import styles from '@/components/article/article.module.css'
import { PageHead } from '@/components/article/page-head'
import { PostList } from '@/components/blog/post-list'
import { wideContainer } from '@/components/landing/section'
import { Footer } from '@/components/site/footer'
import { SiteHeader } from '@/components/site/site-header'
import { buttonVariants } from '@/components/ui/button'
import { getPosts } from '@/lib/blog'
import { pageMetadata } from '@/lib/seo'
import { cn } from '@/lib/utils'

const TITLE = 'iii blog'
const DESCRIPTION = 'Notes from the team building iii — three primitives, zero integration cost.'
const RSS_PATH = '/blog/rss.xml'

const base = pageMetadata({ title: TITLE, description: DESCRIPTION, path: '/blog/' })
export const metadata: Metadata = {
  ...base,
  alternates: { ...base.alternates, types: { 'application/rss+xml': RSS_PATH } },
}

export default function BlogIndex() {
  const posts = getPosts()

  return (
    <>
      <SiteHeader />
      <main>
        <section aria-labelledby="blog-title" className={cn(wideContainer, styles.page)}>
          <div className="flex flex-wrap items-end justify-between gap-x-8 gap-y-6">
            <PageHead eyebrow="Blog" title="Notes from the team building iii." titleId="blog-title" />
            <a href={RSS_PATH} className={cn(buttonVariants({ variant: 'outline' }), 'h-9 px-3')}>
              RSS feed
            </a>
          </div>

          <div className="mt-12 sm:mt-16">
            <PostList posts={posts} />
          </div>
        </section>
      </main>
      <Footer container={wideContainer} />
    </>
  )
}
