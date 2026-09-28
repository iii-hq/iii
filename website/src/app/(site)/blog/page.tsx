import type { Metadata } from "next"
import { PostList } from "@/components/blog/post-list"
import { PageShell } from "@/components/site/page-shell"
import { buttonVariants } from "@/components/ui/button-variants"
import { getPosts } from "@/lib/blog"
import { cn } from "@/lib/utils"

const TITLE = "iii blog"
const DESCRIPTION = "Notes from the team building iii — three primitives, zero integration cost."
const RSS_PATH = "/blog/rss.xml"

export const metadata: Metadata = {
  title: TITLE,
  description: DESCRIPTION,
  alternates: {
    canonical: "/blog/",
    types: { "application/rss+xml": RSS_PATH },
  },
  openGraph: {
    type: "website",
    url: "/blog/",
    title: TITLE,
    description: DESCRIPTION,
    images: [{ url: "/og-image.png", width: 1200, height: 630, type: "image/png" }],
  },
  twitter: {
    card: "summary_large_image",
    title: TITLE,
    description: DESCRIPTION,
    images: ["/og-image.png"],
  },
}

export default function BlogIndex() {
  const posts = getPosts()

  return (
    <PageShell>
      <section
        aria-labelledby="blog-title"
        className="mx-auto max-w-[1200px] px-5 pt-12 pb-20 sm:pt-16 sm:pb-24 md:px-6"
      >
        <header className="flex flex-wrap items-end justify-between gap-x-8 gap-y-5">
          <div className="max-w-[640px]">
            <p className="text-[13px] font-medium tracking-[0.02em] text-gray-10">Blog</p>
            {/* biome-ignore lint/correctness/useUniqueElementIds: single page heading; the section is labelled by it */}
            <h1
              id="blog-title"
              className="mt-3 text-[clamp(28px,3.4vw,40px)] leading-[1.1] font-medium tracking-[-0.03em] text-balance text-gray-12"
            >
              Notes from the team building iii.
            </h1>
          </div>
          <a href={RSS_PATH} className={cn(buttonVariants({ variant: "outline", size: "sm" }), "mb-0.5")}>
            RSS feed
          </a>
        </header>

        <div className="mt-10 sm:mt-14">
          <PostList posts={posts} />
        </div>
      </section>
    </PageShell>
  )
}
