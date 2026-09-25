"use client"

import { ChevronDownIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { formatCount, formatStars, useCommunityStats } from "@/hooks/use-community-stats"
import { trackCta } from "@/lib/analytics"
import { cn } from "@/lib/utils"
import { type SocialLink, socialLinks } from "./nav-links"
import { SocialIcon } from "./social-icon"

/** Live counts replace the static descriptions once the shared stats store loads. */
function useSocialDescription() {
  const { stars, members } = useCommunityStats()
  return (link: SocialLink) => {
    if (link.id === "github" && stars) return `${formatStars(stars)} stars`
    if (link.id === "discord" && members) return `${formatCount(members)} members`
    return link.description
  }
}

/** "Community" menu: GitHub, Discord, X and LinkedIn with icon, title and a one-line description. */
export function SocialsMenu({ className }: { className?: string }) {
  const describe = useSocialDescription()

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className={cn(
          buttonVariants({ variant: "ghost", size: "sm" }),
          "group/menu gap-1 pr-2 text-gray-11 data-popup-open:bg-gray-3 data-popup-open:text-gray-12",
          className,
        )}
      >
        Community
        <ChevronDownIcon className="size-3.5 opacity-70 transition-transform duration-200 ease-out group-data-popup-open/menu:rotate-180" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={8} className="w-[248px]">
        {socialLinks.map((link) => (
          <DropdownMenuItem
            key={link.id}
            className="cursor-pointer gap-3 rounded-[8px] px-2 py-2 focus:bg-gray-4 data-highlighted:bg-gray-4"
            render={
              <a
                href={link.href}
                target="_blank"
                rel="noopener noreferrer"
                onClick={() => trackCta(link.id, "nav_community_menu", { cta_label: link.title.toLowerCase() })}
              >
                <span className="flex size-8 shrink-0 items-center justify-center rounded-[7px] bg-gray-4 text-gray-12">
                  <SocialIcon id={link.id} className="size-4" />
                </span>
                <span className="flex min-w-0 flex-col">
                  <span className="text-sm font-medium text-gray-12">{link.title}</span>
                  <span className="truncate text-xs text-gray-10 tabular-nums">{describe(link)}</span>
                </span>
              </a>
            }
          />
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
