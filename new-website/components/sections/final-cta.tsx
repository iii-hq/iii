import { ArrowUpRightIcon } from 'lucide-react'

import { IconDiscord, IconGitHub } from '@/components/icons/iconly'
import { CopyCommand } from '@/components/site/copy-command'
import { EmailSignup } from '@/components/site/email-signup'
import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { sectionTitleClass } from '@/components/site/section'
import { SectionRule } from '@/components/site/section-rule'
import { type CommunityStats, getCommunityStats } from '@/lib/community'
import { installCommand, links } from '@/lib/site'
import { cn } from '@/lib/utils'

/**
 * Closing CTA, laid out like iii.dev's: copy on the left; on the right the install command, an updates signup,
 * and GitHub + Discord with who is online right now.
 */
export async function FinalCta() {
  const stats = await getCommunityStats()
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: These are stable anchors for the single closing CTA.
    <section id="final-cta" aria-labelledby="final-cta-title" className="relative">
      <SectionRule />
      <div className="mx-auto grid max-w-[1240px] items-start gap-10 px-5 py-16 sm:px-6 md:px-5 md:py-20 lg:grid-cols-[minmax(0,1fr)_minmax(0,480px)] lg:gap-20">
        <Reveal>
          {/* biome-ignore lint/correctness/useUniqueElementIds: The single closing CTA owns this heading anchor. */}
          <PixelHeading id="final-cta-title" className={cn(sectionTitleClass, 'mt-0 sm:mt-0')}>
            Connect your first worker.
          </PixelHeading>
          <p className="mt-4 max-w-xl text-pretty text-base text-muted-foreground leading-relaxed sm:text-[15px]">
            Start with one function, in a language you already use, next to the code you have. Every worker you add
            after it, agents included, joins the same registry and the same trace.
          </p>
          <p className="mt-6 font-mono text-[12px] text-muted-foreground">Self-hosted · No account needed</p>
        </Reveal>
        <Reveal delay={0.1} className="flex min-w-0 flex-col gap-3">
          <CopyCommand command={installCommand} className="rounded-xl" />
          <EmailSignup />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-[auto_minmax(0,1fr)]">
            <a
              href={links.github}
              className="group/gh pressable flex h-11 items-center gap-2.5 rounded-xl border bg-card px-3.5 text-[14px] outline-none transition-colors hover:bg-faint focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
            >
              <IconGitHub className="size-[18px]" />
              <span className="font-medium">GitHub</span>
              {stats.stars ? (
                <span className="font-mono text-[12px] text-muted-foreground tabular-nums">{stats.stars}</span>
              ) : null}
              <ArrowUpRightIcon
                aria-hidden
                className="ml-auto size-3.5 text-muted-foreground transition-transform duration-150 group-hover/gh:translate-x-px group-hover/gh:-translate-y-px motion-reduce:transition-none"
              />
            </a>
            <DiscordLink stats={stats} />
          </div>
        </Reveal>
      </div>
    </section>
  )
}

function DiscordLink({ stats }: { stats: CommunityStats }) {
  return (
    <a
      href={links.discord}
      aria-label={`Join Discord${stats.online ? `, ${stats.online} online now` : ''}`}
      className="group/dc pressable flex h-11 min-w-0 items-center gap-2.5 overflow-hidden rounded-xl border bg-card px-3.5 text-[14px] outline-none transition-colors hover:bg-faint focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
    >
      <IconDiscord className="size-[18px] shrink-0" />
      <span className="font-medium">Discord</span>
      {stats.online ? (
        <span className="flex items-center gap-1.5 border-l pl-2.5 font-mono text-[12px] text-muted-foreground tabular-nums">
          <span className="relative flex size-1.5">
            <span className="absolute inline-flex size-full animate-ping rounded-full bg-ok opacity-60 motion-reduce:animate-none" />
            <span className="relative inline-flex size-1.5 rounded-full bg-ok" />
          </span>
          <span className="text-foreground">{stats.online}</span> online
        </span>
      ) : null}
      {stats.avatars.length ? (
        <span aria-hidden className="ml-auto flex shrink-0 -space-x-1.5 pl-2">
          {stats.avatars.slice(0, 4).map((src) => (
            // biome-ignore lint/performance/noImgElement: remote Discord CDN avatars, already sized at 32px.
            <img
              key={src}
              src={src}
              alt=""
              width={20}
              height={20}
              loading="lazy"
              className="size-5 rounded-full border-2 border-card bg-muted object-cover transition-colors group-hover/dc:border-faint"
            />
          ))}
        </span>
      ) : null}
    </a>
  )
}
