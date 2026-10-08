'use client'

import { motion } from 'motion/react'
import { useEffect, useId, useState } from 'react'

import { spring } from '@/lib/motion'
import styles from './article.module.css'

export type TocItem = { id: string; text: string; depth?: 2 | 3 }

export type TocGroup = {
  /** small label over the group; omit on the first group */
  label?: string
  items: TocItem[]
}

/** Headings at or above this line (the floating header plus a little room) count as passed. */
const LINE = 120

/**
 * A contents list beside a long document. The last heading to pass the top of the viewport is the current one: its
 * label goes to the foreground and one marker slides to it, the same indicator the manifesto's reading index uses.
 */
export function TocRail({ title = 'On this page', groups }: { title?: string; groups: TocGroup[] }) {
  const layoutId = useId()
  const key = groups.flatMap((g) => g.items.map((i) => i.id)).join('\n')
  const [active, setActive] = useState<string | null>(null)

  useEffect(() => {
    const headings = key
      .split('\n')
      .map((id) => document.getElementById(id))
      .filter((el): el is HTMLElement => el !== null)
    if (!headings.length) return
    let frame = 0
    const pick = () => {
      frame = 0
      let current = headings[0].id
      for (const h of headings) {
        if (h.getBoundingClientRect().top <= LINE) current = h.id
        else break
      }
      setActive(current)
    }
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(pick)
    }
    pick()
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll)
    return () => {
      if (frame) cancelAnimationFrame(frame)
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onScroll)
    }
  }, [key])

  if (!key) return null

  return (
    <nav aria-label={title} className={styles.toc}>
      <p className={`${styles.eyebrow} ${styles.tocTitle}`}>{title}</p>
      {groups.map((group, gi) => (
        <div key={group.label ?? gi}>
          {group.label ? <p className={styles.tocGroupLabel}>{group.label}</p> : null}
          <ul>
            {group.items.map((item) => {
              const current = item.id === active
              return (
                <li key={item.id}>
                  <a
                    href={`#${item.id}`}
                    aria-current={current ? 'location' : undefined}
                    data-depth={item.depth ?? 2}
                    className={styles.tocLink}
                  >
                    {current ? (
                      <motion.span layoutId={layoutId} transition={spring.snappy} className={styles.tocMarker} />
                    ) : null}
                    {item.text}
                  </a>
                </li>
              )
            })}
          </ul>
        </div>
      ))}
    </nav>
  )
}
