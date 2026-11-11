import {
  ArrowDownToLineIcon,
  ArrowRightIcon,
  ArrowUpRightIcon,
  CodeXmlIcon,
  DatabaseIcon,
  GlobeIcon,
  HardDriveIcon,
  type LucideIcon,
  MessageSquareIcon,
  PlusIcon,
  SquareKanbanIcon,
  SquareTerminalIcon,
} from 'lucide-react'
import Image from 'next/image'

import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { SectionRule } from '@/components/site/section-rule'
import { TrackedLink } from '@/components/site/tracked-link'
import { buttonVariants } from '@/components/ui/button'
import { stagger } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { type AppShot, appShots } from './app-media'
import { iiiApp } from './content'
import styles from './iii-app.module.css'
import { wideContainer } from './section'

/* The app's own launcher icons (it draws Lucide), so the tiles read as the shelf on the Home screen. */
const appIcons: Record<(typeof iiiApp.apps)[number]['id'], LucideIcon> = {
  chat: MessageSquareIcon,
  tmux: SquareTerminalIcon,
  ide: CodeXmlIcon,
  database: DatabaseIcon,
  storage: HardDriveIcon,
}
const registryIcons: Record<(typeof iiiApp.registry.apps)[number], LucideIcon> = {
  Kanban: SquareKanbanIcon,
  Browser: GlobeIcon,
}

const ctaBase = 'group h-12 w-full gap-2.5 rounded-xl px-5 text-[15px] sm:h-11 sm:w-auto'

/**
 * The iii app, right under the hero: the launch pitch for people who drive Claude Code and Codex from a terminal.
 * Same anatomy as every landing section (rule, eyebrow, pixel heading, lede in one <header>, which
 * scripts/generate-llms-agents.ts reads), then the two calls to action beside a device stage drawn in CSS (a Mac
 * window and two phones, screenshots from ./app-media), and the five built-in apps with the registry's as a
 * hairline grid of the app's own launcher tiles.
 */
export function IiiApp() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <section id="app" aria-labelledby="app-title" className="landing-section relative overflow-x-clip">
      <SectionRule />
      <div className={cn(wideContainer, 'py-16 sm:py-20 md:py-24 lg:py-28')}>
        <div className="grid items-center gap-12 sm:gap-14 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-12 xl:grid-cols-[minmax(0,9fr)_minmax(0,15fr)] xl:gap-16">
          <div className="flex min-w-0 flex-col">
            <Reveal as="header" className="max-w-3xl">
              <p className="font-medium font-sans text-muted-foreground text-xs uppercase leading-none tracking-[0.08em]">
                {iiiApp.eyebrow}
              </p>
              {/* biome-ignore lint/correctness/useUniqueElementIds: The section's single heading anchor. */}
              <PixelHeading
                id="app-title"
                className="mt-4 text-[clamp(2.25rem,10.4vw,2.75rem)] leading-[1.06] tracking-[-0.015em] sm:mt-5 sm:text-[48px] lg:text-[44px] xl:text-[54px] 2xl:text-[60px]"
              >
                {iiiApp.title.map((line) => (
                  <span key={line} className="block whitespace-nowrap">
                    {line}
                  </span>
                ))}
              </PixelHeading>
              <div className="mt-5 max-w-[560px] text-pretty text-base text-muted-foreground leading-relaxed sm:text-[17px]">
                {iiiApp.copy}
              </div>
            </Reveal>
            <Reveal
              delay={0.1}
              className="mt-8 flex w-full max-w-sm flex-col items-stretch gap-3 sm:max-w-none sm:flex-row sm:items-center"
            >
              <TrackedLink
                href={iiiApp.primary.href}
                params={{ cta_id: 'iii_app_download_mac', cta_location: 'landing_app' }}
                className={cn(buttonVariants(), ctaBase, 'pr-4')}
              >
                {iiiApp.primary.label}
                <ArrowDownToLineIcon
                  aria-hidden
                  strokeWidth={1.75}
                  className="size-4 transition-transform duration-150 group-hover:translate-y-0.5 motion-reduce:transition-none"
                />
              </TrackedLink>
              <TrackedLink
                href={iiiApp.secondary.href}
                params={{ cta_id: 'iii_app_page', cta_location: 'landing_app' }}
                className={cn(
                  buttonVariants({ variant: 'outline' }),
                  ctaBase,
                  'pr-4 text-foreground/90 hover:text-foreground dark:bg-transparent dark:hover:bg-faint',
                )}
              >
                {iiiApp.secondary.label}
                <ArrowUpRightIcon
                  aria-hidden
                  strokeWidth={1.75}
                  className="size-4 text-muted-foreground transition-[translate,color] duration-150 group-hover:translate-x-px group-hover:-translate-y-px group-hover:text-foreground motion-reduce:transition-none"
                />
              </TrackedLink>
            </Reveal>
            <Reveal delay={0.15}>
              <p className="mt-4 max-w-[480px] text-pretty font-sans text-[13px] text-muted-foreground leading-relaxed">
                {iiiApp.platforms}
              </p>
            </Reveal>
          </div>

          <DeviceStage />
        </div>

        <Reveal delay={0.1} className="mt-14 sm:mt-16 lg:mt-20">
          <ul className="grid gap-px overflow-hidden rounded-xl border bg-border sm:grid-cols-2 lg:grid-cols-3">
            {iiiApp.apps.map((app) => {
              const Icon = appIcons[app.id]
              return (
                <li key={app.id} className="flex min-w-0 flex-col gap-3.5 bg-card p-5 sm:p-6">
                  <div className="flex min-w-0 items-center gap-3">
                    <span aria-hidden className={styles.tile}>
                      <Icon strokeWidth={1.75} className="size-[18px]" />
                    </span>
                    <h3 className="font-medium text-[16px] text-foreground leading-tight tracking-[-0.01em]">
                      {app.name}
                    </h3>
                    <span className="ml-auto truncate rounded-full px-2 py-[3px] font-mono text-[11px] text-muted-foreground ring-1 ring-border ring-inset">
                      {app.tag}
                    </span>
                  </div>
                  <p className="text-pretty text-[14px] text-muted-foreground leading-relaxed">{app.copy}</p>
                </li>
              )
            })}
            <li className="flex min-w-0 flex-col gap-3.5 bg-card p-5 sm:p-6">
              <div className="flex min-w-0 items-center gap-3">
                <span aria-hidden className={cn(styles.tile, styles.tileAdd)}>
                  <PlusIcon strokeWidth={1.75} className="size-[18px]" />
                </span>
                <h3 className="font-medium text-[16px] text-foreground leading-tight tracking-[-0.01em]">
                  {iiiApp.registry.title}
                </h3>
              </div>
              <p className="text-pretty text-[14px] text-muted-foreground leading-relaxed">{iiiApp.registry.copy}</p>
              <div className="mt-auto flex flex-wrap items-center gap-2 pt-1">
                {iiiApp.registry.apps.map((name) => {
                  const Icon = registryIcons[name]
                  return (
                    <span
                      key={name}
                      className="inline-flex h-7 items-center gap-1.5 rounded-full bg-faint pr-2.5 pl-2 text-[12.5px] text-foreground/90 ring-1 ring-border ring-inset"
                    >
                      <Icon aria-hidden strokeWidth={1.75} className="size-3.5 text-muted-foreground" />
                      {name}
                    </span>
                  )
                })}
                <a
                  href={iiiApp.registry.link.href}
                  className="group/registry ml-auto inline-flex h-7 items-center gap-1 rounded-sm text-[13px] text-foreground/80 outline-none transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
                >
                  {iiiApp.registry.link.label}
                  <ArrowRightIcon
                    aria-hidden
                    strokeWidth={1.75}
                    className="size-3.5 transition-transform duration-150 group-hover/registry:translate-x-0.5 motion-reduce:transition-none"
                  />
                </a>
              </div>
            </li>
          </ul>
        </Reveal>
      </div>
    </section>
  )
}

/** The Mac window and the two phones, each fading up in turn; reduced motion shows them in place. */
function DeviceStage() {
  return (
    <div className={styles.stageFrame}>
      <div className={styles.stage}>
        <div aria-hidden className={styles.glow} />
        <div aria-hidden className={cn(styles.dots, 'bg-dots')} />
        <div aria-hidden className={styles.floor} />
        <Reveal className={cn(styles.device, styles.mac)}>
          <figure className={styles.window}>
            <div aria-hidden className={styles.titleBar}>
              <span className={styles.light} />
              <span className={styles.light} />
              <span className={styles.light} />
              <span className={styles.titleText}>iii</span>
            </div>
            <Screen shot={appShots.mac} className={styles.macScreen} />
          </figure>
        </Reveal>
        <Reveal delay={0.12 + stagger} className={cn(styles.device, styles.android)}>
          <figure className={cn(styles.phone, styles.androidPhone)}>
            <Screen shot={appShots.android} />
          </figure>
        </Reveal>
        <Reveal delay={0.12} className={cn(styles.device, styles.iphone)}>
          <figure className={styles.phone}>
            <Screen shot={appShots.iphone} />
          </figure>
        </Reveal>
      </div>
    </div>
  )
}

function Screen({ shot, className }: { shot: AppShot; className?: string }) {
  return (
    <div className={cn(styles.screen, className)}>
      <Image src={shot.src} alt={shot.alt} width={shot.width} height={shot.height} loading="lazy" />
    </div>
  )
}
