'use client'

import { IconGitHub, IconStar } from '@/components/icons/iconly'
import { buttonVariants } from '@/components/ui/button'
import { useLiveCounts } from '@/hooks/use-live-community-stats'
import { cn } from '@/lib/utils'

/** GitHub's own starred-repo yellow (dark theme). Same token as the header star count. */
const githubStar = 'group-hover:text-[#e3b341] group-focus-visible:text-[#e3b341]'

/**
 * "Star on GitHub" with the star count in the button: the build-time count first, then the live one from the
 * browser (useLiveCounts, the same reading the site header shows).
 */
export function StarButton({
  href,
  label,
  stars: buildStars,
  className,
}: {
  href: string
  label: string
  stars: number | null
  className?: string
}) {
  const { stars } = useLiveCounts({ stars: buildStars, members: null })
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className={cn(buttonVariants(), className, stars !== null && 'pr-3.5')}
    >
      <IconGitHub className="size-4" />
      {label}
      {stars !== null ? (
        <span className="ml-1 flex items-center gap-1 border-primary-foreground/20 border-l pl-3 text-primary-foreground/75 tabular-nums">
          <IconStar
            className={cn(
              'size-3.5 transition-[color,rotate,scale] duration-300 ease-out motion-safe:group-hover:rotate-[72deg] motion-safe:group-hover:scale-110',
              githubStar,
            )}
          />
          {stars.toLocaleString('en-US')}
          <span className="sr-only"> stars</span>
        </span>
      ) : null}
    </a>
  )
}
