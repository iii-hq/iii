import type { Metadata } from 'next'
import { notFound } from 'next/navigation'

import styles from '@/components/article/article.module.css'
import { PageHead } from '@/components/article/page-head'
import { Prose } from '@/components/article/prose'
import { TocRail } from '@/components/article/toc-rail'
import { PostImage } from '@/components/blog/post-image'
import { PostNav } from '@/components/blog/post-nav'
import { wideContainer } from '@/components/landing/section'
import { Footer } from '@/components/site/footer'
import { JsonLd } from '@/components/site/json-ld'
import { SiteHeader } from '@/components/site/site-header'
import { formatDate, getPost, getPosts, publicImage } from '@/lib/blog'
import { isoDateTime, postOgImage, postUrl, splitBanner } from '@/lib/blog-post'
import { publicImageSize } from '@/lib/image-size'
import { renderMarkdown } from '@/lib/markdown'
import { faqJsonLd, site } from '@/lib/seo'
import { links } from '@/lib/site'
import { cn } from '@/lib/utils'

type Params = { slug: string }

export const dynamicParams = false

export function generateStaticParams(): Params[] {
  return getPosts().map(({ slug }) => ({ slug }))
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params
  const post = getPost(slug)
  if (!post) return {}

  const url = postUrl(slug)
  const image = postOgImage(post)
  const size = post.image ? publicImageSize(post.image) : { width: 1200, height: 630 }
  const title = post.seoTitle ?? post.title

  return {
    title: { absolute: title },
    keywords: post.tags.length ? post.tags : undefined,
    description: post.description,
    authors: post.author ? [{ name: post.author }] : undefined,
    alternates: {
      canonical: url,
      types: { 'application/rss+xml': '/blog/rss.xml' },
    },
    openGraph: {
      type: 'article',
      siteName: site.name,
      locale: site.locale,
      url,
      title,
      description: post.description,
      publishedTime: isoDateTime(post.date),
      modifiedTime: post.updated ? isoDateTime(post.updated) : undefined,
      authors: post.author ? [post.author] : undefined,
      tags: post.tags,
      images: [{ url: image, ...(size ?? {}) }],
    },
    twitter: {
      card: 'summary_large_image',
      site: site.twitter,
      creator: site.twitter,
      title,
      description: post.description,
      images: [image],
    },
  }
}

/** Posts with fewer h2s than this also list their h3s, so the outline stays useful. */
const FEW_HEADINGS = 3

export default async function BlogPost({ params }: { params: Promise<Params> }) {
  const { slug } = await params
  const posts = getPosts()
  const index = posts.findIndex((p) => p.slug === slug)
  const post = posts[index]
  if (!post) notFound()

  // `posts` is newest first: the next entry is older, the previous is newer.
  const older = posts[index + 1]
  const newer = index > 0 ? posts[index - 1] : undefined

  const { banner, body } = splitBanner(post.body)
  const { content, headings } = await renderMarkdown(body, { image: publicImage })
  const h2s = headings.filter((h) => h.depth === 2)
  const candidates = h2s.length < FEW_HEADINGS ? headings : h2s
  const outline = candidates.length >= 2 ? candidates : []
  const url = postUrl(slug)

  const publisher = {
    '@type': 'Organization',
    name: 'III, Inc.',
    url: `${site.url}/`,
    logo: { '@type': 'ImageObject', url: `${site.url}/favicon.svg` },
  }

  // A post with no named author is written by the team: the publisher is the author too, so the entry is never
  // without one. Keywords, word count and language give answer engines the same facets the page shows.
  const jsonLd = {
    '@context': 'https://schema.org',
    '@type': 'BlogPosting',
    headline: post.title,
    ...(post.seoTitle ? { alternativeHeadline: post.seoTitle } : {}),
    description: post.description,
    datePublished: isoDateTime(post.date),
    dateModified: isoDateTime(post.updated ?? post.date),
    author: post.author ? { '@type': 'Person', name: post.author } : publisher,
    publisher,
    image: postOgImage(post),
    url,
    mainEntityOfPage: url,
    inLanguage: 'en',
    isAccessibleForFree: true,
    wordCount: post.wordCount,
    ...(post.tags.length ? { keywords: post.tags.join(', ') } : {}),
  }

  return (
    <>
      <SiteHeader />
      <main>
        <div className={cn(wideContainer, styles.page, styles.layout, styles.layoutTocStart)}>
          <article aria-labelledby="post-title" className={styles.main}>
            <PageHead eyebrow="Blog" eyebrowHref={links.blog} title={post.title} titleId="post-title">
              <p className={styles.meta}>
                <time dateTime={post.date}>{formatDate(post.date)}</time>
                {post.author ? (
                  <>
                    <span aria-hidden="true">·</span>
                    <span>{post.author}</span>
                  </>
                ) : null}
                <span aria-hidden="true">·</span>
                <span>{post.readingTime} min read</span>
                {post.updated ? (
                  <>
                    <span aria-hidden="true">·</span>
                    <span>
                      Updated <time dateTime={post.updated}>{formatDate(post.updated)}</time>
                    </span>
                  </>
                ) : null}
              </p>
            </PageHead>

            {banner ? (
              <PostImage
                src={banner.src}
                alt={banner.alt}
                priority
                sizes="(min-width: 800px) 760px, 90vw"
                className="mt-12"
              />
            ) : null}

            <Prose className="mt-14">{content}</Prose>

            <PostNav older={older} newer={newer} />
          </article>

          <aside className={styles.aside}>
            <TocRail groups={[{ items: outline }]} />
          </aside>
        </div>
      </main>
      <Footer container={wideContainer} />
      <JsonLd data={jsonLd} />
      {post.faq.length ? <JsonLd data={faqJsonLd(post.faq)} /> : null}
    </>
  )
}
