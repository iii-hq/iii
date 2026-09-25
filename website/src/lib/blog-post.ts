import "server-only"

import { type Post, publicImage } from "./blog"
import { site } from "./site"

export type Banner = { src: string; alt: string }

/** Absolute URL of a post. */
export const postUrl = (slug: string) => `${site.url}/blog/${slug}/`

/** Absolute URL for link previews: the post's own banner, else the site card. */
export const postOgImage = (post: Post) => `${site.url}${post.image ?? "/og-image.png"}`

/** "2026-05-07" → "2026-05-07T00:00:00.000Z", the form the Astro layout emitted. */
export const isoDateTime = (date: string) => new Date(`${date}T00:00:00Z`).toISOString()

/**
 * Most posts open with their banner as the first inline image. The post page
 * renders that banner itself, above the body, so pull it out of the markdown
 * to avoid showing it twice. Posts that don't start with an image have no banner.
 */
export function splitBanner(body: string): { banner: Banner | null; body: string } {
  const m = body.match(/^\s*!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)[ \t]*\r?\n/)
  if (!m) return { banner: null, body }
  return { banner: { src: publicImage(m[2]), alt: m[1] }, body: body.slice(m[0].length) }
}
