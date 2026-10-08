'use client'

import { NavigationMenu } from '@base-ui/react/navigation-menu'
import { ArrowRightIcon, ArrowUpRightIcon } from 'lucide-react'
import { AnimatePresence, motion, useMotionValueEvent, useScroll } from 'motion/react'
import { useEffect, useState } from 'react'

import {
  IconArrowDown,
  IconBookmark,
  IconDiscord,
  IconDiscovery,
  IconDocument,
  IconGitHub,
  IconLinkedIn,
  IconPackage,
  IconPaper,
  IconStar,
  IconX,
} from '@/components/icons/iconly'
import { buttonVariants } from '@/components/ui/button'
import type { CommunityStats } from '@/lib/community'
import { duration, easeOut, spring, stagger } from '@/lib/motion'
import { links } from '@/lib/site'
import { cn } from '@/lib/utils'
import { CountUp } from './count-up'
import { NavLogo } from './nav-graphics'

type Icon = React.ComponentType<{ className?: string }>
/** `meta` is a short right-aligned detail (a live count); empty when there is nothing worth saying. */
type MenuLink = { title: string; meta: string; href: string; icon: Icon }

function communityLinks(stats: CommunityStats): MenuLink[] {
  return [
    { title: 'GitHub', meta: stats.stars ?? '', href: links.github, icon: IconGitHub },
    { title: 'Discord', meta: stats.members ?? '', href: links.discord, icon: IconDiscord },
    { title: 'X', meta: '', href: links.x, icon: IconX },
    { title: 'LinkedIn', meta: '', href: links.linkedin, icon: IconLinkedIn },
  ]
}

/** Same destinations, in the same order, as the current iii.dev header. */
const plainLinks = [
  { label: 'Manifesto', href: links.manifesto, icon: IconBookmark, external: false },
  { label: 'Docs', href: links.docs, icon: IconDocument, external: false },
  { label: 'Blog', href: links.blog, icon: IconPaper, external: false },
  { label: 'Roadmap', href: links.roadmap, icon: IconDiscovery, external: false },
  { label: 'Worker registry', href: links.registry, icon: IconPackage, external: true },
]

const focusRing = 'outline-none focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2'

/** Resting nav text: one step brighter than `muted-foreground` so the links read without competing with the page. */
const navText = 'text-foreground/75'

/** GitHub's own starred-repo yellow (dark theme). */
const githubStar = 'group-hover:text-[#e3b341] group-focus-visible:text-[#e3b341]'

export function Header({ stats }: { stats: CommunityStats }) {
  const { scrollY } = useScroll()
  const [condensed, setCondensed] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)

  useMotionValueEvent(scrollY, 'change', (y) => setCondensed(y > 24))

  useEffect(() => {
    if (!mobileOpen) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setMobileOpen(false)
    const onResize = () => window.innerWidth >= 1024 && setMobileOpen(false)
    window.addEventListener('keydown', onKey)
    window.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('resize', onResize)
    }
  }, [mobileOpen])

  const floating = condensed || mobileOpen
  const community = communityLinks(stats)

  return (
    <>
      <AnimatePresence>
        {mobileOpen ? (
          <motion.div
            key="menu-backdrop"
            aria-hidden
            onClick={() => setMobileOpen(false)}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: duration.slow }}
            className="fixed inset-0 z-40 bg-background/60 backdrop-blur-sm lg:hidden"
          />
        ) : null}
      </AnimatePresence>
      {/* Phones: a 20px gutter, the same as the page content, so the floating bar lines up with the column under it. */}
      <header className="pointer-events-none fixed inset-x-0 top-0 z-50 px-5 md:px-4">
        <motion.div
          initial={false}
          animate={{
            maxWidth: floating ? 1080 : 1240,
            marginTop: floating ? 10 : 0,
            borderRadius: floating ? 16 : 0,
          }}
          transition={spring.soft}
          className={cn(
            'pointer-events-auto relative mx-auto overflow-hidden border transition-[background-color,border-color,box-shadow,backdrop-filter] duration-300',
            floating
              ? 'border-border bg-background/70 shadow-[0_8px_30px_-14px_rgb(0_0_0/0.14)] backdrop-blur-xl backdrop-saturate-150 dark:bg-[oklch(0.16_0_0/0.72)] dark:shadow-[0_8px_32px_-12px_rgb(0_0_0/0.5),inset_0_1px_0_0_rgb(255_255_255/0.04)]'
              : 'border-transparent bg-transparent',
          )}
        >
          {/* Phones: a 48px bar, and the logo and the menu icon sit the same 14px in from each end (the icon is 16px
              inside a 36px button, so 4px of padding puts its lines 14px in). Wider screens keep the 56px bar. */}
          <div className="flex h-12 items-center gap-2 pr-1 pl-3.5 sm:h-14 sm:pr-2 sm:pl-4 md:pl-5">
            <a href="/" aria-label="iii home" className={cn('mr-3 flex items-center rounded-md', focusRing)}>
              <NavLogo />
            </a>

            <DesktopNav community={community} />

            <div className="ml-auto flex items-center gap-1">
              <StatLink
                href={links.github}
                label="GitHub stars"
                value={stats.stars}
                count={stats.starsCount}
                format="plain"
                icon={IconGitHub}
                star
              />
              <StatLink
                href={links.discord}
                label="Discord members"
                value={stats.members}
                count={stats.membersCount}
                format="compact"
                delay={0.45}
                icon={IconDiscord}
              />
              <a
                href={links.install}
                className={cn(
                  buttonVariants(),
                  'group ml-1.5 hidden h-8 rounded-xl pr-2.5 pl-3 text-[13px] xl:inline-flex',
                )}
              >
                Install iii
                <ArrowRightIcon className="size-3.5 transition-transform duration-150 group-hover:translate-x-0.5" />
              </a>
              <MenuButton open={mobileOpen} onToggle={() => setMobileOpen((o) => !o)} />
            </div>
          </div>

          <AnimatePresence initial={false}>
            {mobileOpen ? <MobilePanel community={community} onNavigate={() => setMobileOpen(false)} /> : null}
          </AnimatePresence>
        </motion.div>
      </header>
    </>
  )
}

/** Icon + live count (GitHub stars, Discord members) that ticks up on first paint. Hidden when the count couldn't be loaded. */
function StatLink({
  href,
  label,
  value,
  count,
  format,
  delay,
  icon: Icon,
  star = false,
}: {
  href: string
  label: string
  value: string | null
  count: number | null
  format: 'plain' | 'compact'
  delay?: number
  icon: Icon
  /** Prefix the count with a star that turns GitHub yellow on hover (the GitHub stars link). */
  star?: boolean
}) {
  if (!value) return null
  return (
    <a
      href={href}
      aria-label={`${label}: ${value}`}
      className={cn(
        'group hidden h-8 items-center gap-1.5 rounded-lg px-2 text-[13px] tabular-nums transition-colors hover:bg-foreground/[0.05] hover:text-foreground lg:inline-flex',
        navText,
        focusRing,
      )}
    >
      <Icon className="size-4 text-foreground/85 transition-[color,transform] duration-200 ease-out group-hover:scale-105 group-hover:text-foreground" />
      <span className="flex items-center gap-1">
        {star ? (
          // A fifth of a turn lands the star back on its own outline, so it reads as a twinkle, not a spin.
          <IconStar
            className={cn(
              'size-3.5 text-foreground/55 transition-[color,rotate,scale] duration-300 ease-out motion-safe:group-hover:rotate-[72deg] motion-safe:group-hover:scale-110',
              githubStar,
            )}
          />
        ) : null}
        {count === null ? value : <CountUp value={count} format={format} delay={delay} />}
      </span>
    </a>
  )
}

function DesktopNav({ community }: { community: MenuLink[] }) {
  const [hovered, setHovered] = useState<string | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const pill = hovered ?? open

  return (
    <NavigationMenu.Root
      value={open}
      onValueChange={(value) => setOpen((value as string | null) ?? null)}
      className="hidden lg:block"
    >
      <NavigationMenu.List className="flex items-center" onMouseLeave={() => setHovered(null)}>
        {plainLinks.map((item) => (
          <NavItem key={item.label} id={item.label} pill={pill} onHover={setHovered}>
            <NavigationMenu.Link href={item.href} className={triggerClass}>
              <span className="relative z-10">{item.label}</span>
              {item.external ? (
                <ArrowUpRightIcon className="relative z-10 size-3 opacity-50 transition-transform duration-150 group-hover:translate-x-px group-hover:-translate-y-px" />
              ) : null}
            </NavigationMenu.Link>
          </NavItem>
        ))}

        <NavItem id="community" pill={pill} onHover={setHovered}>
          <NavigationMenu.Trigger className={triggerClass}>
            <span className="relative z-10">Community</span>
            <IconArrowDown className="relative z-10 size-3.5 opacity-50 transition-transform duration-200 ease-out group-data-[popup-open]:rotate-180" />
          </NavigationMenu.Trigger>
          <NavigationMenu.Content className={contentClass}>
            <CommunityPanel items={community} />
          </NavigationMenu.Content>
        </NavItem>
      </NavigationMenu.List>

      <NavigationMenu.Portal>
        <NavigationMenu.Positioner
          sideOffset={10}
          align="start"
          alignOffset={-4}
          collisionPadding={16}
          className="z-[60] h-(--positioner-height) w-(--positioner-width) max-w-(--available-width) transition-[top,left,right,bottom] duration-200 ease-out before:absolute before:inset-x-0 before:-top-3 before:h-3 data-instant:transition-none"
        >
          <NavigationMenu.Popup className="relative h-(--popup-height) w-(--popup-width) origin-(--transform-origin) overflow-hidden rounded-xl border bg-popover/95 shadow-[0_12px_32px_-12px_rgb(0_0_0/0.25)] backdrop-blur-xl transition-[opacity,transform,width,height,scale] duration-200 ease-out data-[ending-style]:scale-[0.98] data-[starting-style]:scale-[0.98] data-[ending-style]:opacity-0 data-[starting-style]:opacity-0 data-[ending-style]:duration-150 dark:shadow-[0_12px_32px_-12px_rgb(0_0_0/0.6),inset_0_1px_0_0_rgb(255_255_255/0.05)]">
            <NavigationMenu.Viewport className="relative size-full overflow-hidden" />
          </NavigationMenu.Popup>
        </NavigationMenu.Positioner>
      </NavigationMenu.Portal>
    </NavigationMenu.Root>
  )
}

const triggerClass = cn(
  'group relative inline-flex h-8 items-center gap-1 rounded-lg px-2.5 text-[13px] transition-colors hover:text-foreground data-[popup-open]:text-foreground',
  navText,
  focusRing,
)

const contentClass =
  'h-full w-auto transition-[opacity,translate] duration-200 ease-out data-[starting-style]:opacity-0 data-[ending-style]:opacity-0 data-[starting-style]:data-[activation-direction=left]:-translate-x-2 data-[starting-style]:data-[activation-direction=right]:translate-x-2 data-[ending-style]:data-[activation-direction=left]:translate-x-2 data-[ending-style]:data-[activation-direction=right]:-translate-x-2'

function NavItem({
  id,
  pill,
  onHover,
  children,
}: {
  id: string
  pill: string | null
  onHover: (id: string) => void
  children: React.ReactNode
}) {
  return (
    <NavigationMenu.Item value={id} className="relative" onMouseEnter={() => onHover(id)} onFocus={() => onHover(id)}>
      {pill === id ? (
        <motion.span
          layoutId="nav-hover-pill"
          aria-hidden
          className="absolute inset-0 rounded-lg bg-foreground/[0.06] dark:bg-foreground/[0.08]"
          transition={spring.snappy}
        />
      ) : null}
      {children}
    </NavigationMenu.Item>
  )
}

/** One compact row: 16px icon, label, and a right-aligned mono count when there is one. */
function MenuRow({ item, index }: { item: MenuLink; index: number }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: duration.base, delay: 0.04 + index * stagger, ease: easeOut }}
    >
      <NavigationMenu.Link
        href={item.href}
        className={cn(
          'group/row flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-foreground transition-colors hover:bg-foreground/[0.05] focus-visible:bg-foreground/[0.05]',
          focusRing,
        )}
      >
        <item.icon className="size-4 text-foreground/75 transition-colors group-hover/row:text-foreground" />
        <span className="flex-1 truncate">{item.title}</span>
        {item.meta ? (
          <span className="font-sans text-[11.5px] text-foreground/75 tabular-nums">{item.meta}</span>
        ) : null}
      </NavigationMenu.Link>
    </motion.div>
  )
}

function CommunityPanel({ items }: { items: MenuLink[] }) {
  return (
    <div className="flex w-48 flex-col gap-px p-1">
      {items.map((item, index) => (
        <MenuRow key={item.title} item={item} index={index} />
      ))}
    </div>
  )
}

function MenuButton({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      aria-label={open ? 'Close menu' : 'Open menu'}
      aria-expanded={open}
      aria-controls="mobile-menu"
      onClick={onToggle}
      className={cn(
        'pressable relative ml-1 flex size-9 items-center justify-center rounded-lg text-foreground hover:bg-foreground/[0.06] lg:hidden',
        focusRing,
      )}
    >
      <motion.span
        aria-hidden
        className="absolute h-[1.5px] w-4 rounded-full bg-current"
        initial={false}
        animate={open ? { rotate: 45, y: 0 } : { rotate: 0, y: -3.5 }}
        transition={{ duration: duration.slow, ease: easeOut }}
      />
      <motion.span
        aria-hidden
        className="absolute h-[1.5px] w-4 rounded-full bg-current"
        initial={false}
        animate={open ? { rotate: -45, y: 0 } : { rotate: 0, y: 3.5 }}
        transition={{ duration: duration.slow, ease: easeOut }}
      />
    </button>
  )
}

function MobilePanel({ community, onNavigate }: { community: MenuLink[]; onNavigate: () => void }) {
  const groups: { title: string; items: MenuLink[] }[] = [
    { title: 'Resources', items: plainLinks.map((l) => ({ title: l.label, meta: '', href: l.href, icon: l.icon })) },
    { title: 'Community', items: community },
  ]
  let index = 0
  return (
    <motion.nav
      id="mobile-menu"
      aria-label="Mobile"
      initial={{ height: 0, opacity: 0 }}
      animate={{ height: 'auto', opacity: 1 }}
      exit={{ height: 0, opacity: 0 }}
      transition={{ duration: 0.4, ease: easeOut }}
      className="overflow-hidden lg:hidden"
    >
      <div className="max-h-[calc(100svh-96px)] overflow-y-auto border-t px-1.5 pt-2 pb-3">
        {groups.map((group) => (
          <div key={group.title} className="py-2">
            <p className="px-2 pb-1 font-sans text-[11px] text-muted-foreground uppercase tracking-[0.08em]">
              {group.title}
            </p>
            {group.items.map((item) => {
              const delay = stagger * 0.7 * index++
              return (
                <motion.a
                  key={item.title}
                  href={item.href}
                  onClick={onNavigate}
                  initial={{ opacity: 0, y: -6 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ duration: 0.35, delay: 0.08 + delay, ease: easeOut }}
                  className={cn(
                    'flex items-center gap-3 rounded-lg px-2 py-2.5 text-[15px] transition-colors focus-visible:bg-foreground/[0.06] active:bg-foreground/[0.06]',
                    focusRing,
                  )}
                >
                  <item.icon className="size-[18px] text-foreground/80" />
                  {item.title}
                  {item.meta ? (
                    <span className="ml-auto font-sans text-muted-foreground text-xs tabular-nums">{item.meta}</span>
                  ) : null}
                </motion.a>
              )
            })}
          </div>
        ))}
        <motion.div
          initial={{ opacity: 0, y: -6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.35, delay: 0.1 + stagger * 0.7 * index, ease: easeOut }}
          className="flex items-center gap-2 px-1 pt-2"
        >
          <a href={links.install} className={cn(buttonVariants(), 'h-10 flex-1')}>
            Install iii
          </a>
        </motion.div>
      </div>
    </motion.nav>
  )
}
