import type { Metadata } from "next"
import { PageShell } from "@/components/site/page-shell"
import { Prose } from "@/components/site/prose"
import { TocRail } from "@/components/site/toc-rail"
import { renderMarkdown } from "@/lib/markdown"
import { body, privacy } from "./privacy-data"

const description =
  "Privacy policy for the iii.dev website, operated by Motia LLC. What we collect, how we use it, the analytics providers we use, and your choices."
const ogTitle = "iii / privacy policy"

export const metadata: Metadata = {
  title: ogTitle,
  description,
  keywords: [
    "iii",
    "privacy policy",
    "data protection",
    "website analytics",
    "cookies",
    "data security",
    "GDPR",
    "developer privacy",
  ],
  authors: [{ name: "Motia LLC" }],
  alternates: { canonical: "/privacy-policy" },
  openGraph: {
    type: "article",
    url: "/privacy-policy",
    title: ogTitle,
    description,
    images: [{ url: "/og-image.png", width: 1200, height: 630, type: "image/png" }],
  },
  twitter: {
    card: "summary_large_image",
    title: ogTitle,
    description,
    images: ["/og-image.png"],
  },
}

/**
 * The policy as one article: a compact page head, the ten sections in
 * `<Prose>`, and on wide screens a sticky contents list in the trailing column.
 */
export default async function PrivacyPolicyPage() {
  const { content, headings } = await renderMarkdown(body)
  const sections = headings.filter((h) => h.depth === 2)

  return (
    <PageShell>
      {/* Two columns from the top: the head and the article share the first, the
          contents list holds the second, so "Contents" sits level with the eyebrow. */}
      <div className="mx-auto grid max-w-[1200px] gap-x-16 px-5 pt-14 pb-20 sm:pt-20 sm:pb-24 md:px-6 lg:grid-cols-[minmax(0,1fr)_220px] xl:gap-x-24">
        <header className="max-w-[640px]">
          <p className="text-[13px] font-medium tracking-[0.02em] text-gray-10">{privacy.eyebrow}</p>
          {/* biome-ignore lint/correctness/useUniqueElementIds: the article's aria-labelledby targets this id; it renders once */}
          <h1
            id="privacy-title"
            className="mt-3 text-[clamp(28px,3.4vw,40px)] leading-[1.1] font-medium tracking-[-0.03em] text-balance text-gray-12"
          >
            {privacy.title}
          </h1>
          <p className="mt-4 max-w-[560px] text-[15px] leading-[1.6] text-pretty text-gray-11">{privacy.lede}</p>
          <p className="mt-4 text-[13px] text-gray-10">Last updated {privacy.updated}</p>
        </header>

        {/* Wide screens only: the list would only push the policy down on narrow ones. */}
        <aside className="hidden lg:row-span-2 lg:block">
          <TocRail groups={[{ items: sections }]} className="sticky top-24" />
        </aside>

        <article aria-labelledby="privacy-title" className="mt-12 min-w-0 sm:mt-16">
          <Prose className="[&_.doc-updated]:mt-6 [&_.doc-updated]:text-[13px] [&_.doc-updated]:text-gray-10">
            {content}
          </Prose>
        </article>
      </div>
    </PageShell>
  )
}
