'use client'

import { Maximize2Icon, XIcon } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { type ComponentType, type ReactNode, useCallback, useEffect, useRef, useState } from 'react'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import { IconBot, IconCategory, IconServer } from '@/components/icons/iconly'
import { Reveal } from '@/components/site/reveal'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { easeOut } from '@/lib/motion'
import { useCases } from './content'
import { Section } from './section'
import styles from './use-cases.module.css'
import { HarnessVisual, InfraVisual, PlatformVisual } from './use-cases-demos'
import { useSpinnerFrame } from './use-cases-motion'

/**
 * Three ways to use iii, side by side (2026-10-05 sync: no graph, no problem subtitles, no links yet; "Custom
 * harness"; App platform shows several workloads; Infrastructure reproduces the `iii compose` renderer from
 * iii-hq/iii#2263). No window chrome, so this does not read as one more copy of the app panels above it (Mike,
 * 00:05:38). 2026-10-07 sync (Anthony): a way to focus on one card at a time; each card expands on its own, which
 * also makes clear the three are separate things, not steps of one flow.
 */

type CardId = 'harness' | 'platform' | 'infra'
type VisualProps = { running: boolean; frame: string; still: boolean }

const CARDS: {
  id: CardId
  icon: ReactNode
  title: string
  copy: string
  tag: string
  mono?: boolean
  Visual: ComponentType<VisualProps>
}[] = [
  {
    id: 'harness',
    icon: <IconBot className="size-4" />,
    title: useCases.harness.label,
    copy: useCases.harness.solution,
    tag: 'Your choices',
    Visual: HarnessVisual,
  },
  {
    id: 'platform',
    icon: <IconCategory className="size-4" />,
    title: useCases.platform.label,
    copy: useCases.platform.solution,
    tag: `${useCases.platform.workloads.length} workloads`,
    Visual: PlatformVisual,
  },
  {
    id: 'infra',
    icon: <IconServer className="size-4" />,
    title: useCases.infra.label,
    copy: useCases.infra.solution,
    tag: useCases.infra.terminal.command,
    mono: true,
    Visual: InfraVisual,
  },
]

export function UseCases() {
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const [focused, setFocused] = useState<CardId | null>(null)
  /* The cards behind the focused one pause; the focused one plays on its own clock from the start. */
  const gridRunning = running && focused === null
  const frame = useSpinnerFrame(gridRunning || (focused !== null && !paused && !reduce))
  const openerRef = useRef<HTMLButtonElement | null>(null)
  const closeFocus = useCallback(() => {
    setFocused(null)
    openerRef.current?.focus()
  }, [])

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="use-cases" eyebrow={useCases.eyebrow} title={useCases.title} lede={useCases.subtitle}>
      <Reveal delay={0.1} className="mt-10 lg:mt-14">
        <div ref={ref}>
          <div className={styles.grid}>
            {CARDS.map((card) => (
              <article key={card.id} className={styles.card}>
                <div className={styles.visual}>
                  <card.Visual running={gridRunning} frame={frame} still={reduce} />
                  <button
                    type="button"
                    className={styles.expand}
                    aria-label={`Focus on ${card.title}`}
                    onClick={(event) => {
                      openerRef.current = event.currentTarget
                      setFocused(card.id)
                    }}
                  >
                    <Maximize2Icon aria-hidden strokeWidth={1.75} className="size-3.5" />
                  </button>
                </div>
                <CardText card={card} />
              </article>
            ))}
          </div>
          <div className={styles.controls}>
            <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
          </div>
        </div>
      </Reveal>

      <AnimatePresence>
        {focused ? (
          <FocusDialog
            key={focused}
            card={CARDS.find((card) => card.id === focused) ?? CARDS[0]}
            running={!paused && !reduce}
            frame={frame}
            still={reduce}
            onClose={closeFocus}
          />
        ) : null}
      </AnimatePresence>
    </Section>
  )
}

function CardText({ card }: { card: (typeof CARDS)[number] }) {
  return (
    <div className={styles.text}>
      <div className={styles.titleRow}>
        <span className={styles.cardIcon}>{card.icon}</span>
        <h3 className={styles.cardTitle}>{card.title}</h3>
        <span className={styles.tag} data-mono={card.mono ?? false}>
          {card.tag}
        </span>
      </div>
      <p className={styles.copy}>{card.copy}</p>
    </div>
  )
}

/**
 * One card on its own, larger. A modal, so it stays centred (Emil: modals are exempt from origin-aware scaling):
 * the backdrop fades, the panel rises from 0.96 with a little blur, and leaves faster than it came. Escape, the
 * close button or the backdrop dismiss it; focus moves to the close button and returns to the card's button after.
 */
function FocusDialog({
  card,
  running,
  frame,
  still,
  onClose,
}: {
  card: (typeof CARDS)[number]
  running: boolean
  frame: string
  still: boolean
  onClose: () => void
}) {
  const closeRef = useRef<HTMLButtonElement>(null)
  /* Runs once per open: focus in, Escape out, page scroll locked until it closes. */
  useEffect(() => {
    closeRef.current?.focus()
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    const root = document.documentElement
    const overflow = root.style.overflow
    root.style.overflow = 'hidden'
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('keydown', onKey)
      root.style.overflow = overflow
    }
  }, [onClose])

  return (
    <div className={styles.dialogRoot}>
      <motion.div
        className={styles.backdrop}
        onClick={onClose}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0, transition: { duration: 0.15 } }}
        transition={{ duration: 0.2, ease: 'easeOut' }}
      />
      <motion.div
        role="dialog"
        aria-modal="true"
        aria-label={card.title}
        className={styles.dialog}
        initial={{ opacity: 0, transform: 'scale(0.96)', filter: 'blur(4px)' }}
        animate={{ opacity: 1, transform: 'scale(1)', filter: 'blur(0px)' }}
        exit={{ opacity: 0, transform: 'scale(0.98)', filter: 'blur(2px)', transition: { duration: 0.15 } }}
        transition={{ duration: 0.26, ease: easeOut }}
      >
        <div className={styles.dialogVisual}>
          <card.Visual running={running} frame={frame} still={still} />
        </div>
        <div className={styles.dialogText}>
          <CardText card={card} />
          <button ref={closeRef} type="button" onClick={onClose} className={styles.close} aria-label="Close">
            <XIcon aria-hidden strokeWidth={1.75} className="size-4" />
          </button>
        </div>
      </motion.div>
    </div>
  )
}
