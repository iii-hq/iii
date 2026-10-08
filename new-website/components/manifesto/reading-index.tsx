'use client'

import { motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { spring } from '@/lib/motion'
import type { Statement } from './data'
import styles from './manifesto.module.css'

/**
 * The twelve statements as a sticky index beside the text. The one being read is marked by a single indicator that
 * slides between rows (no per-row blink), and the rows already read stay a step brighter than the ones ahead.
 */
export function ReadingIndex({ statements }: { statements: Statement[] }) {
  const [active, setActive] = useState(statements[0]?.id)

  useEffect(() => {
    const visible = new Map<string, number>()
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries)
          visible.set(entry.target.id, entry.isIntersecting ? entry.boundingClientRect.top : Infinity)
        /* The statement nearest the reading line (40% down the viewport) wins. */
        const line = window.innerHeight * 0.4
        let best: string | undefined
        let bestDistance = Infinity
        for (const [id, top] of visible) {
          if (top === Infinity) continue
          const distance = Math.abs(top - line)
          if (distance < bestDistance) {
            best = id
            bestDistance = distance
          }
        }
        if (best) setActive(best)
      },
      { rootMargin: '-15% 0px -45% 0px' },
    )
    for (const statement of statements) {
      const element = document.getElementById(statement.id)
      if (element) observer.observe(element)
    }
    return () => observer.disconnect()
  }, [statements])

  const activeIndex = statements.findIndex((statement) => statement.id === active)
  return (
    <nav aria-label="Manifesto statements" className={styles.index}>
      <ol>
        {statements.map((statement, i) => (
          <li key={statement.id}>
            <a
              href={`#${statement.id}`}
              aria-current={statement.id === active ? 'true' : undefined}
              data-read={i < activeIndex}
              className={styles.indexLink}
            >
              {statement.id === active ? (
                <motion.span layoutId="manifesto-index" transition={spring.snappy} className={styles.indexMarker} />
              ) : null}
              <span className={styles.indexText}>
                {statement.code ? <code>{statement.accent}</code> : statement.accent} {statement.heavy}
              </span>
            </a>
          </li>
        ))}
      </ol>
    </nav>
  )
}
