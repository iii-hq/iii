import Image from "next/image"
import { LogoMark } from "@/components/site/logo"
import { SiteLink } from "@/components/site/site-link"
import { formatDate, type Post } from "@/lib/blog"

/**
 * A category label from the tags. Every post is tagged "agents", so that one is
 * skipped in favour of the first tag that tells posts apart ("architecture",
 * "compliance", "sandbox"). Hyphenated tags read as words; short ones as acronyms.
 */
function category(post: Post) {
  const tag = post.tags.find((t) => t !== "agents") ?? post.tags[0]
  if (!tag) return "Post"
  return tag
    .split("-")
    .map((w) => (w.length <= 3 ? w.toUpperCase() : w[0].toUpperCase() + w.slice(1)))
    .join(" ")
}

/**
 * The blog index body: a grid of cards, newest first. Each card is one link:
 * the whole banner fitted in a 16:10 tile, the title, then category and
 * date. Posts without a banner get a quiet tile with the mark, so the grid
 * never has a hole. Hover lifts the image a hair and brightens the title;
 * both are transform/colour only.
 */
export function PostList({ posts }: { posts: Post[] }) {
  return (
    <ol className="grid gap-x-6 gap-y-12 sm:grid-cols-2 lg:grid-cols-3">
      {posts.map((post, i) => (
        <li key={post.slug}>
          <SiteLink
            href={`/blog/${post.slug}`}
            className="group/card -m-3 flex flex-col gap-4 rounded-[18px] p-3 outline-none focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1"
          >
            {/* A taller tile, and the banner is fitted inside it rather than cropped, so the whole
                image is always visible whatever shape it was exported at. */}
            <div className="relative aspect-[16/10] overflow-hidden rounded-[14px] bg-gray-3 shadow-[inset_0_0_0_1px_var(--gray-5)]">
              {post.image ? (
                <Image
                  src={post.image}
                  alt=""
                  fill
                  priority={i < 3}
                  sizes="(min-width: 1024px) 384px, (min-width: 640px) 50vw, 100vw"
                  className="object-contain p-3 transition-transform duration-500 ease-[cubic-bezier(0.23,1,0.32,1)] group-hover/card:scale-[1.02] motion-reduce:transition-none"
                />
              ) : (
                <div className="absolute inset-0 flex items-center justify-center">
                  <LogoMark className="size-9 text-gray-8" />
                </div>
              )}
              {/* Hairline on top of the image so the crop edge is crisp in both themes. */}
              <div
                aria-hidden="true"
                className="pointer-events-none absolute inset-0 rounded-[14px] shadow-[inset_0_0_0_1px_var(--line)]"
              />
            </div>

            <div className="min-w-0 px-0.5">
              <h2 className="text-[18px] leading-[1.3] font-medium tracking-[-0.015em] text-balance text-gray-12">
                {post.title}
              </h2>
              <p className="mt-2 flex flex-wrap items-center gap-x-2.5 text-[13px] leading-[1.5] text-gray-10">
                <span className="font-medium text-gray-12">{category(post)}</span>
                <time dateTime={post.date} className="tabular-nums">
                  {formatDate(post.date)}
                </time>
                <span aria-hidden="true">·</span>
                <span>{post.readingTime} min read</span>
              </p>
            </div>
          </SiteLink>
        </li>
      ))}
    </ol>
  )
}
