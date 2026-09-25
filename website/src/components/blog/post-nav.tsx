import { ArrowRightIcon } from "@/components/site/iconly"
import { SiteLink } from "@/components/site/site-link"
import { buttonVariants } from "@/components/ui/button-variants"
import type { Post } from "@/lib/blog"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"

type PostNavProps = {
  /** the post published before this one */
  older?: Post
  /** the post published after this one */
  newer?: Post
}

const LINK = cn(
  "group/nav -mx-4 flex min-w-0 flex-col gap-1 rounded-[12px] px-4 py-3 outline-none",
  "transition-[background-color] duration-150 ease-out hover:bg-gray-3",
  "focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
)

function NavLink({ post, label, align }: { post: Post; label: string; align: "start" | "end" }) {
  const end = align === "end"
  return (
    <SiteLink href={`/blog/${post.slug}`} className={cn(LINK, end && "sm:items-end sm:text-right")}>
      <span className="flex items-center gap-1.5 text-[13px] text-gray-10">
        {!end && (
          <ArrowRightIcon className="size-3.5 rotate-180 transition-transform duration-150 ease-out group-hover/nav:-translate-x-0.5" />
        )}
        {label}
        {end && (
          <ArrowRightIcon className="size-3.5 transition-transform duration-150 ease-out group-hover/nav:translate-x-0.5" />
        )}
      </span>
      <span className="text-[15px] leading-[1.45] font-medium tracking-[-0.01em] text-pretty text-gray-12">
        {post.title}
      </span>
    </SiteLink>
  )
}

/** Older / newer post links and the way back to the index, after the article body. */
export function PostNav({ older, newer }: PostNavProps) {
  return (
    <nav aria-label="More posts" className="mt-16 border-t border-gray-5 pt-8">
      {(older || newer) && (
        <div className="grid gap-4 sm:grid-cols-2 sm:gap-8">
          {older ? <NavLink post={older} label="Older" align="start" /> : <span aria-hidden="true" />}
          {newer && <NavLink post={newer} label="Newer" align="end" />}
        </div>
      )}
      <div className={cn(older || newer ? "mt-6" : "")}>
        <SiteLink href={links.blog} className={cn(buttonVariants({ variant: "ghost" }), "-mx-3.5 text-gray-12")}>
          All posts
        </SiteLink>
      </div>
    </nav>
  )
}
