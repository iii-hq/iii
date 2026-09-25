"use client"

import { AnimatePresence, m, useInView, useReducedMotion } from "motion/react"
import { useEffect, useRef, useState } from "react"
import { Swap } from "@/components/motion/swap"
import { DiscordIcon } from "@/components/site/icons"
import { buttonVariants } from "@/components/ui/button-variants"
import { type DiscordMember, formatCount, useCommunityStats } from "@/hooks/use-community-stats"
import { trackCta } from "@/lib/analytics"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"

// Avatar stack geometry: 24px circles overlapping on a 17px step, 4 visible.
const SIZE = 24
const STEP = 17
const VIS_MAX = 4
const CYCLE_MS = 2400
const SLIDE = { duration: 0.6, ease: [0.4, 0, 0.2, 1] } as const
const FADE = { duration: 0.45, ease: [0.25, 0.1, 0.25, 1] } as const

/** Discord CTA on the get-started card: "N online" + a cycling avatar stack. */
export function DiscordButton({ className, size = "lg" }: { className?: string; size?: "default" | "lg" }) {
  const { online, avatars } = useCommunityStats()
  const loading = online === undefined
  const failed = online === null

  return (
    <a
      href={links.discord}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={online ? `Join Discord, ${formatCount(online)} online` : "Join Discord"}
      onClick={() => trackCta("discord", "footer", { cta_label: "discord" })}
      className={cn(buttonVariants({ variant: "outline", size }), "gap-3", className)}
    >
      <DiscordIcon className="size-4" />
      Discord
      {!failed && (
        <>
          <span aria-hidden="true" className="h-4 w-px bg-gray-6" />
          <span
            className={cn(
              "inline-flex items-center gap-1.5 text-[13px] font-normal text-gray-11 tabular-nums transition-opacity duration-150 ease-out",
              loading && "opacity-50",
            )}
          >
            {/* Presence dot: green with a slow ring, like Discord's own. Grey while the count loads. */}
            <span aria-hidden="true" className="relative flex size-1.5">
              {!loading && (
                <span className="absolute inset-0 rounded-full bg-online animate-ping [animation-duration:2.4s] motion-reduce:hidden" />
              )}
              <span className={cn("relative size-1.5 rounded-full", loading ? "bg-gray-8" : "bg-online")} />
            </span>
            <Swap id={String(online ?? "loading")}>{online != null ? formatCount(online) : "—"}</Swap> online
          </span>
          <AvatarStack members={avatars} />
        </>
      )}
    </a>
  )
}

function initials(name: string) {
  if (!name) return "?"
  const s = name.replace(/[^a-zA-Z0-9 ]/g, "").trim()
  const parts = s.split(/\s+/)
  if (parts.length >= 2) return parts[0][0] + parts[1][0]
  return s.slice(0, 2)
}

/**
 * Up to four overlapping avatars. Every 2.4s the leftmost slides out, the rest
 * shift left and the next member slides in on the right. Pauses off screen and
 * holds still under reduced m.
 */
function AvatarStack({ members }: { members: DiscordMember[] }) {
  const ref = useRef<HTMLSpanElement>(null)
  const inView = useInView(ref)
  const reduce = useReducedMotion()
  // `head` counts cycles; slot keys are head + i, so each arrival is a new element.
  const [head, setHead] = useState(0)
  const count = members.length
  const visible = Math.min(VIS_MAX, count)
  const cycling = count > visible && inView && !reduce

  useEffect(() => {
    if (!cycling) return
    const timer = setInterval(() => setHead((h) => h + 1), CYCLE_MS)
    return () => clearInterval(timer)
  }, [cycling])

  return (
    <span
      ref={ref}
      aria-hidden="true"
      className="relative inline-block h-6 overflow-hidden align-middle max-sm:hidden"
      style={{ width: visible ? (visible - 1) * STEP + SIZE : 0 }}
    >
      {/* Remount when data first arrives so the initial fill doesn't animate in. */}
      <AnimatePresence key={count ? "ready" : "empty"} initial={false}>
        {Array.from({ length: visible }, (_, i) => {
          const seq = head + i
          const member = members[seq % count]
          return (
            <m.span
              key={seq}
              title={member.username}
              initial={{ x: visible * STEP, opacity: 0 }}
              animate={{ x: i * STEP, opacity: 1 }}
              exit={{ x: -STEP, opacity: 0 }}
              transition={{ x: SLIDE, opacity: FADE }}
              style={member.avatarUrl ? { backgroundImage: `url("${member.avatarUrl}")` } : undefined}
              className="absolute top-0 left-0 inline-flex size-6 items-center justify-center overflow-hidden rounded-full bg-gray-4 bg-cover bg-center text-[10px] font-semibold text-gray-12 uppercase ring-2 ring-gray-2"
            >
              {member.avatarUrl ? null : initials(member.username)}
            </m.span>
          )
        })}
      </AnimatePresence>
    </span>
  )
}
