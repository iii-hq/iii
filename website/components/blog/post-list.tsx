import Image from 'next/image'

import { Logo } from '@/components/site/logo'
import { formatDate, type Post } from '@/lib/blog'

/**
 * A category label from the tags. Every post is tagged "agents", so that one is skipped in favour of the first tag
 * that tells posts apart ("architecture", "compliance", "sandbox"). Hyphenated tags read as words; short ones as
 * acronyms.
 */
function category(post: Post) {
  const tag = post.tags.find((t) => t !== 'agents') ?? post.tags[0]
  if (!tag) return 'Post'
  return tag
    .split('-')
    .map((w) => (w.length <= 3 ? w.toUpperCase() : w[0].toUpperCase() + w.slice(1)))
    .join(' ')
}

/**
 * The blog index body: a grid of cards, newest first. Each card is one link: the whole banner fitted in a 16:10 tile,
 * the title, then category and date. Posts without a banner get a quiet tile with the mark, so the grid never has a
 * hole. Hover lifts the image a hair and brightens the tile's edge; both are transform and colour only.
 */
export function PostList({ posts }: { posts: Post[] }) {
  return (
    <ol className="grid gap-x-6 gap-y-12 sm:grid-cols-2 lg:grid-cols-3">
      {posts.map((post, i) => (
        <li key={post.slug}>
          <a
            href={`/blog/${post.slug}`}
            className="group/card -m-3 flex flex-col gap-4 rounded-2xl p-3 outline-none focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
          >
            <div className="relative aspect-[16/10] overflow-hidden rounded-xl border bg-card transition-colors group-hover/card:border-line-strong">
              {post.image ? (
                <Image
                  src={post.image}
                  alt=""
                  fill
                  priority={i < 3}
                  sizes="(min-width: 1024px) 33vw, (min-width: 640px) 50vw, 100vw"
                  className="object-contain p-3 transition-transform duration-500 ease-out group-hover/card:scale-[1.02] motion-reduce:transition-none"
                />
              ) : (
                <div className="absolute inset-0 flex items-center justify-center">
                  <Logo className="h-8 text-line-strong" />
                </div>
              )}
            </div>

            <div className="min-w-0 px-0.5">
              <h2 className="text-balance font-medium text-[18px] text-foreground leading-[1.3] tracking-[-0.015em]">
                {post.title}
              </h2>
              <p className="mt-2 flex flex-wrap items-center gap-x-2.5 text-[13px] text-muted-foreground leading-normal">
                <span className="font-medium text-foreground/80">{category(post)}</span>
                <time dateTime={post.date} className="tabular-nums">
                  {formatDate(post.date)}
                </time>
                <span aria-hidden="true">·</span>
                <span>{post.readingTime} min read</span>
              </p>
            </div>
          </a>
        </li>
      ))}
    </ol>
  )
}
