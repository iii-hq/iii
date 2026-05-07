import rss from '@astrojs/rss'
import { getCollection } from 'astro:content'
import type { APIContext } from 'astro'

export async function GET(context: APIContext) {
  const posts = await getCollection('blog', ({ data }) => !data.draft)
  // The feed lives at /blog/rss.xml with items under /blog/<slug>/.
  // `site` becomes the channel <link>; it must be the blog home, not the
  // apex site, or readers treat the feed as describing iii.dev instead of /blog.
  const origin = context.site ?? new URL('https://iii.dev/')
  const site = new URL('blog/', origin).href
  const selfLink = new URL('rss.xml', site).href

  return rss({
    title: 'iii blog',
    description: 'Notes from the team building iii — three primitives, zero integration cost.',
    site,
    xmlns: {
      atom: 'http://www.w3.org/2005/Atom',
    },
    customData: `<atom:link href="${selfLink}" rel="self" type="application/rss+xml"/>`,
    items: posts
      .sort((a, b) => b.data.pubDate.valueOf() - a.data.pubDate.valueOf())
      .map((post) => ({
        title: post.data.title,
        description: post.data.description,
        pubDate: post.data.pubDate,
        link: `/blog/${post.id}/`,
      })),
  })
}
