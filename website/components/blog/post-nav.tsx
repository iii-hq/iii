import { ArrowLeftIcon, ArrowRightIcon } from 'lucide-react'

import { buttonVariants } from '@/components/ui/button'
import type { Post } from '@/lib/blog'
import { links } from '@/lib/site'
import { cn } from '@/lib/utils'

type PostNavProps = {
  /** the post published before this one */
  older?: Post
  /** the post published after this one */
  newer?: Post
}

const cell =
  'group/nav -mx-4 flex min-w-0 flex-col gap-1 rounded-xl px-4 py-3 outline-none transition-colors hover:bg-foreground/[0.04] focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2'

function NavLink({ post, label, align }: { post: Post; label: string; align: 'start' | 'end' }) {
  const end = align === 'end'
  return (
    <a href={`/blog/${post.slug}`} className={cn(cell, end && 'sm:items-end sm:text-right')}>
      <span className="flex items-center gap-1.5 text-[13px] text-muted-foreground">
        {end ? null : (
          <ArrowLeftIcon
            aria-hidden
            strokeWidth={1.75}
            className="size-3.5 transition-transform group-hover/nav:-translate-x-0.5"
          />
        )}
        {label}
        {end ? (
          <ArrowRightIcon
            aria-hidden
            strokeWidth={1.75}
            className="size-3.5 transition-transform group-hover/nav:translate-x-0.5"
          />
        ) : null}
      </span>
      <span className="text-pretty font-medium text-[15px] text-foreground leading-[1.45] tracking-[-0.01em]">
        {post.title}
      </span>
    </a>
  )
}

/** Older / newer post links and the way back to the index, after the article body. */
export function PostNav({ older, newer }: PostNavProps) {
  return (
    <nav aria-label="More posts" className="mt-16 border-t pt-8">
      {older || newer ? (
        <div className="grid gap-4 sm:grid-cols-2 sm:gap-8">
          {older ? <NavLink post={older} label="Older" align="start" /> : <span aria-hidden="true" />}
          {newer ? <NavLink post={newer} label="Newer" align="end" /> : null}
        </div>
      ) : null}
      <div className={older || newer ? 'mt-6' : undefined}>
        <a href={links.blog} className={cn(buttonVariants({ variant: 'ghost' }), '-mx-2.5 h-9 text-foreground')}>
          All posts
        </a>
      </div>
    </nav>
  )
}
