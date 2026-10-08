import { jsonLdHtml } from '@/lib/seo'

/** One JSON-LD block. The payload is our own static data, escaped by `jsonLdHtml` so it cannot close the tag. */
export function JsonLd({ data }: { data: unknown }) {
  // biome-ignore lint/security/noDangerouslySetInnerHtml: JSON-LD has to be inline; jsonLdHtml escapes `<`.
  return <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLdHtml(data) }} />
}
