'use client'

import { ArrowRightIcon } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useId, useRef, useState } from 'react'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import { PixelHeading } from '@/components/site/pixel-heading'
import { SectionRule } from '@/components/site/section-rule'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { easeOut, spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

import { story } from './content'
import { wideContainer } from './section'
import styles from './story.module.css'
import { StoryActivity } from './story-asides'
import { DiscoverPreview } from './story-discovery'
import { ExtendPreview } from './story-extend'
import { StoryGraph } from './story-graph'
import { lastStoryBeat, STORY_BEATS, STORY_STEPS, type StoryStage } from './story-model'
import { ObserveActivity } from './story-observability'
import { ReactPreview } from './story-react'
import { useStoryPlayback } from './use-story-playback'

/** Explanations lead on the left; the active chapter's system or observability preview sits on the right. */
export function Story() {
  const headingId = useId()
  const [stage, setStage] = useState<StoryStage>('compose')
  const [visibleChapters, setVisibleChapters] = useState<Partial<Record<StoryStage, boolean>>>({})
  const enterChapter = useCallback((next: StoryStage) => {
    setStage(next)
  }, [])
  const chapterVisibility = useCallback((next: StoryStage, visible: boolean) => {
    setVisibleChapters((previous) => ({ ...previous, [next]: visible }))
    // Resize can change which chapter owns the viewport without another enter event.
    const current = chapterAtReadingLine()
    if (current) setStage(current)
  }, [])
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const chapterRunning = running && Boolean(visibleChapters[stage])
  const playback = useStoryPlayback(stage, chapterRunning)
  const beat = reduce ? lastStoryBeat(stage) : playback.beat
  // The first beat animates too (it is the lead-in); only the held finished state is static.
  const animating = chapterRunning && playback.phase !== 'holding'
  /* No step numbers (2026-10-07 sync): a hairline shows how far the chapter has played instead. */
  const progress = reduce ? 1 : (beat + 1) / STORY_BEATS[stage].length
  const controls = (
    <div className={styles.controls} data-playback={reduce ? 'static' : playback.phase}>
      <span aria-hidden className={styles.progress}>
        <span style={{ transform: `scaleX(${progress.toFixed(3)})` }} />
      </span>
      {/* Pause only: every chapter loops on its own, so there is nothing to replay. */}
      <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
    </div>
  )

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One CODER story per page.
    <section id="coder" aria-labelledby={headingId} className={cn('landing-section relative', styles.story)}>
      <SectionRule />
      <div ref={ref} className={wideContainer}>
        <header className={styles.intro}>
          <p className={styles.eyebrow}>{story.eyebrow}</p>
          <PixelHeading id={headingId} className={styles.introTitle}>
            {story.title}
          </PixelHeading>
          <p className={styles.introCopy}>{story.description}</p>
        </header>
        <div className={styles.layout}>
          <ol className={styles.chapters}>
            {STORY_STEPS.map((chapter) => {
              const active = chapter.id === stage
              const chapterBeat = active ? beat : lastStoryBeat(chapter.id)
              return (
                <StoryChapter key={chapter.id} chapter={chapter} onVisibility={chapterVisibility}>
                  <StoryActivity stage={chapter.id} beat={chapterBeat} running={active && animating} />
                  <div className={styles.mobileGraph}>
                    <StoryPreview stage={chapter.id} beat={chapterBeat} running={active && animating} />
                    {active ? controls : null}
                  </div>
                </StoryChapter>
              )
            })}
          </ol>
          <div className={styles.desktopGraph}>
            <div className={styles.stickyGraph}>
              <nav aria-label="Explore CODER" className={styles.chapterNav}>
                {STORY_STEPS.map((chapter) => (
                  <a
                    key={chapter.id}
                    href={`#coder-${chapter.id}`}
                    aria-label={chapter.name}
                    aria-current={stage === chapter.id ? 'step' : undefined}
                    onClick={() => enterChapter(chapter.id)}
                  >
                    <span className={styles.navLetter}>{chapter.letter}</span>
                    <span className={styles.navName}>{chapter.name}</span>
                    {/* One indicator that slides between chapters instead of five that blink on and off. */}
                    {stage === chapter.id ? (
                      <motion.span
                        layoutId="coder-nav-indicator"
                        transition={spring.snappy}
                        className={styles.navIndicator}
                      />
                    ) : null}
                  </a>
                ))}
              </nav>
              {/* Chapters crossfade: the old one fades out fast, the new one rises in with a little blur. The frame
                  keeps one minimum height so the sticky column never jumps as previews of different sizes swap. */}
              <div className={styles.previewFrame}>
                <AnimatePresence mode="popLayout" initial={false}>
                  <motion.div
                    key={stage}
                    initial={{ opacity: 0, transform: 'translateY(8px)', filter: 'blur(4px)' }}
                    animate={{ opacity: 1, transform: 'translateY(0px)', filter: 'blur(0px)' }}
                    exit={{
                      opacity: 0,
                      transform: 'translateY(-4px)',
                      filter: 'blur(2px)',
                      transition: { duration: 0.16 },
                    }}
                    transition={{ duration: 0.32, ease: easeOut }}
                  >
                    <StoryPreview stage={stage} beat={beat} running={animating} />
                  </motion.div>
                </AnimatePresence>
              </div>
              <div className={styles.graphFooter}>{controls}</div>
            </div>
          </div>
        </div>
        {/* The recap: one line, then the five chapters as links back to where each one plays. */}
        <div className={styles.summary}>
          <div className={styles.summaryHead}>
            <p className={styles.eyebrow}>{story.summary.title}</p>
            <p className={styles.summaryLine}>{story.summary.subtitle}</p>
          </div>
          <a href="#proof" className={styles.summaryLink}>
            See it in an agent runtime
            <ArrowRightIcon aria-hidden strokeWidth={1.75} className={styles.summaryArrow} />
          </a>
          <ol className={styles.recap}>
            {STORY_STEPS.map((chapter) => (
              <li key={chapter.id}>
                <a href={`#coder-${chapter.id}`} onClick={() => enterChapter(chapter.id)}>
                  <span className={styles.recapTop}>
                    <span aria-hidden className={styles.recapLetter}>
                      {chapter.letter}
                    </span>
                  </span>
                  <span className={styles.recapName}>{chapter.name}</span>
                  <span className={styles.recapTitle}>{chapter.title}</span>
                </a>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  )
}

function StoryPreview({ stage, beat, running }: { stage: StoryStage; beat: number; running: boolean }) {
  if (stage === 'observe') return <ObserveActivity beat={beat} running={running} />
  if (stage === 'discover') return <DiscoverPreview beat={beat} running={running} />
  if (stage === 'extend') return <ExtendPreview beat={beat} running={running} />
  if (stage === 'react') return <ReactPreview beat={beat} />
  return <StoryGraph stage={stage} beat={beat} running={running} />
}

function StoryChapter({
  chapter,
  onVisibility,
  children,
}: {
  chapter: (typeof STORY_STEPS)[number]
  onVisibility: (stage: StoryStage, visible: boolean) => void
  children: React.ReactNode
}) {
  const ref = useRef<HTMLLIElement>(null)
  useEffect(() => {
    const element = ref.current
    if (!element) return
    const observer = new IntersectionObserver(
      ([entry]) => {
        onVisibility(chapter.id, entry.isIntersecting)
      },
      { rootMargin: '-25% 0px -45% 0px' },
    )
    observer.observe(element)
    return () => observer.disconnect()
  }, [chapter.id, onVisibility])

  return (
    <li ref={ref} id={`coder-${chapter.id}`} className={styles.chapter}>
      <header>
        <p className={styles.eyebrow}>
          <span className={styles.chapterLetter}>{chapter.letter}</span>
          {chapter.name}
        </p>
        <h3 className={cn('font-pixel', styles.chapterTitle)}>{chapter.title}</h3>
        <p className={styles.description}>{chapter.description}</p>
      </header>
      {children}
    </li>
  )
}

function chapterAtReadingLine() {
  const line = window.innerHeight * 0.4
  return STORY_STEPS.find(({ id }) => {
    const bounds = document.getElementById(`coder-${id}`)?.getBoundingClientRect()
    return bounds && bounds.top <= line && bounds.bottom > line
  })?.id
}
