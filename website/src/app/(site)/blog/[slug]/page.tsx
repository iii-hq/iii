import type { Metadata } from "next"
import { notFound } from "next/navigation"
import { PostImage } from "@/components/blog/post-image"
import { PostNav } from "@/components/blog/post-nav"
import { PageShell } from "@/components/site/page-shell"
import { Prose } from "@/components/site/prose"
import { SiteLink } from "@/components/site/site-link"
import { TocRail } from "@/components/site/toc-rail"
import { formatDate, getPost, getPosts, publicImage } from "@/lib/blog"
import { isoDateTime, postOgImage, postUrl, splitBanner } from "@/lib/blog-post"
import { publicImageSize } from "@/lib/image-size"
import { renderMarkdown } from "@/lib/markdown"
import { site } from "@/lib/site"

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

  return {
    title: post.title,
    description: post.description,
    authors: post.author ? [{ name: post.author }] : undefined,
    alternates: {
      canonical: url,
      types: { "application/rss+xml": "/blog/rss.xml" },
    },
    openGraph: {
      type: "article",
      url,
      title: post.title,
      description: post.description,
      publishedTime: isoDateTime(post.date),
      modifiedTime: post.updated ? isoDateTime(post.updated) : undefined,
      authors: post.author ? [post.author] : undefined,
      tags: post.tags,
      images: [{ url: image, ...(size ?? {}) }],
    },
    twitter: {
      card: "summary_large_image",
      title: post.title,
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
  const outline =
    (h2s.length < FEW_HEADINGS ? headings : h2s).length >= 2 ? (h2s.length < FEW_HEADINGS ? headings : h2s) : []
  const url = postUrl(slug)

  // The same BlogPosting the Astro layout emitted.
  const jsonLd = {
    "@context": "https://schema.org",
    "@type": "BlogPosting",
    headline: post.title,
    description: post.description,
    datePublished: isoDateTime(post.date),
    ...(post.updated ? { dateModified: isoDateTime(post.updated) } : {}),
    ...(post.author ? { author: { "@type": "Person", name: post.author } } : {}),
    publisher: {
      "@type": "Organization",
      name: "III, Inc.",
      url: `${site.url}/`,
      logo: { "@type": "ImageObject", url: `${site.url}/favicon.svg` },
    },
    image: postOgImage(post),
    url,
    mainEntityOfPage: url,
  }

  return (
    <PageShell>
      <div className="mx-auto max-w-[1200px] px-5 pt-12 pb-20 sm:pt-16 sm:pb-24 md:px-6 lg:flex lg:items-start">
        <article className="min-w-0 max-w-[640px]">
          <header>
            <p className="text-[13px] font-medium tracking-[0.02em] text-gray-10">
              <SiteLink
                href="/blog"
                className="rounded-[4px] outline-none transition-[color] duration-150 ease-out hover:text-gray-12 focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1"
              >
                Blog
              </SiteLink>
            </p>
            <h1 className="mt-3 text-[clamp(32px,4.4vw,48px)] leading-[1.08] font-medium tracking-[-0.03em] text-balance text-gray-12">
              {post.title}
            </h1>
            <p className="mt-5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[14px] leading-[1.5] text-gray-10">
              <time dateTime={post.date} className="tabular-nums">
                {formatDate(post.date)}
              </time>
              {post.author && (
                <>
                  <span aria-hidden="true">·</span>
                  <span>{post.author}</span>
                </>
              )}
              <span aria-hidden="true">·</span>
              <span>{post.readingTime} min read</span>
              {post.updated && (
                <>
                  <span aria-hidden="true">·</span>
                  <span>
                    Updated{" "}
                    <time dateTime={post.updated} className="tabular-nums">
                      {formatDate(post.updated)}
                    </time>
                  </span>
                </>
              )}
            </p>
          </header>

          {banner && (
            <PostImage
              src={banner.src}
              alt={banner.alt}
              priority
              sizes="(min-width: 640px) 640px, calc(100vw - 40px)"
              className="mt-10"
            />
          )}

          <Prose className="mt-10">{content}</Prose>

          <PostNav older={older} newer={newer} />
        </article>

        <TocRail
          title="On this page"
          groups={[{ items: outline }]}
          className="sticky top-24 ml-auto hidden w-[200px] shrink-0 max-h-[calc(100vh-128px)] overflow-y-auto lg:block"
        />
      </div>

      <script
        type="application/ld+json"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: JSON-LD, escaped below
        dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd).replace(/</g, "\\u003c") }}
      />
    </PageShell>
  )
}
