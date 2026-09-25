"use client"

import { Swap } from "@/components/motion/swap"
import { DiscordIcon, GitHubIcon } from "@/components/site/icons"
import { buttonVariants } from "@/components/ui/button-variants"
import { formatCount, formatStars, useCommunityStats } from "@/hooks/use-community-stats"
import { trackCta } from "@/lib/analytics"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"

const statText = (value: number | null | undefined, format: (n: number) => string) =>
  value == null ? "—" : format(value)

/** GitHub stars + Discord members in the header, counts fading in once the shared stats store loads. */
export function CommunityLinks({ location, className }: { location: string; className?: string }) {
  const { stars, members } = useCommunityStats()
  const item = cn(buttonVariants({ variant: "ghost", size: "sm" }), "gap-1.5 px-2.5 text-gray-11 tabular-nums")

  return (
    <div className={cn("flex items-center", className)}>
      <a
        href={links.github}
        target="_blank"
        rel="noopener noreferrer"
        aria-label={`GitHub repository${stars ? `, ${formatStars(stars)} stars` : ""}`}
        onClick={() => trackCta("github", location, { cta_label: "github" })}
        className={item}
      >
        <GitHubIcon className="size-4" />
        {/* Width reserved for a six-character count, so the nav to the left never moves while it loads. */}
        <Swap id={String(stars ?? "loading")} className="min-w-[6ch]">
          {statText(stars, formatStars)}
        </Swap>
      </a>
      <a
        href={links.discord}
        target="_blank"
        rel="noopener noreferrer"
        aria-label={`Join Discord${members ? `, ${formatCount(members)} members` : ""}`}
        onClick={() => trackCta("discord", location, { cta_label: "discord" })}
        className={item}
      >
        <DiscordIcon className="size-4" />
        <Swap id={String(members ?? "loading")} className="min-w-[3.5ch]">
          {statText(members, formatCount)}
        </Swap>
      </a>
    </div>
  )
}
