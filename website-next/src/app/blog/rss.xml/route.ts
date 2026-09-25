import { getPosts } from "@/lib/blog"
import { site } from "@/lib/site"

// Rendered once at build time; the posts are files in the repo.
export const dynamic = "force-static"

const TITLE = "iii blog"
const DESCRIPTION = "Notes from the team building iii — three primitives, zero integration cost."

const xmlText = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;")

const rfc822 = (iso: string) => new Date(`${iso}T00:00:00Z`).toUTCString()

/**
 * The blog feed: same channel the Astro site served at /blog/rss.xml. The
 * channel link is the blog home, not the apex, so readers describe the feed as
 * /blog rather than iii.dev; items are absolute, newest first.
 */
export function GET() {
  // The channel link keeps its trailing slash (it names the blog "directory");
  // the self link and item links are the URLs this site actually serves.
  const channel = `${site.url}/blog/`
  const self = `${site.url}/blog/rss.xml`

  const items = getPosts()
    .map((post) => {
      const link = `${site.url}/blog/${post.slug}`
      return [
        "<item>",
        `<title>${xmlText(post.title)}</title>`,
        `<link>${link}</link>`,
        `<guid isPermaLink="true">${link}</guid>`,
        `<description>${xmlText(post.description)}</description>`,
        `<pubDate>${rfc822(post.date)}</pubDate>`,
        "</item>",
      ].join("")
    })
    .join("\n")

  const xml = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">',
    "<channel>",
    `<title>${xmlText(TITLE)}</title>`,
    `<description>${xmlText(DESCRIPTION)}</description>`,
    `<link>${channel}</link>`,
    `<atom:link href="${self}" rel="self" type="application/rss+xml"/>`,
    items,
    "</channel>",
    "</rss>",
  ].join("\n")

  return new Response(xml, {
    headers: {
      "Content-Type": "application/rss+xml; charset=utf-8",
      "Cache-Control": "public, max-age=3600, s-maxage=3600, stale-while-revalidate=86400",
    },
  })
}
