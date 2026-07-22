import { glob } from 'astro/loaders'
import { defineCollection, z } from 'astro:content'

const blog = defineCollection({
  loader: glob({ pattern: '**/*.{md,mdx}', base: './src/content/blog' }),
  schema: ({ image }) =>
    z.object({
      title: z.string(),
      description: z.string(),
      pubDate: z.coerce.date(),
      updatedDate: z.coerce.date().optional(),
      draft: z.boolean().optional().default(false),
      tags: z.array(z.string()).optional().default([]),
      author: z.string().optional(),
      // The post's banner image, reused as its og:image / twitter:image link
      // preview instead of the site-wide default. Same relative path as the
      // banner already embedded as the post's first inline image.
      ogImage: image().optional(),
    }),
})

export const collections = { blog }
