import type { ReactNode } from 'react'

import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { cn } from '@/lib/utils'
import styles from './article.module.css'

type PageHeadProps = {
  /** small label over the title; a link back to the parent page when `eyebrowHref` is set */
  eyebrow?: string
  eyebrowHref?: string
  title: ReactNode
  /** id for the h1 so the page region can be labelled by it */
  titleId?: string
  /** the one paragraph under the title */
  description?: ReactNode
  /** anything that follows the paragraph (meta line, actions) */
  children?: ReactNode
  className?: string
}

/**
 * The head of a long-form page, in the same anatomy as the manifesto and roadmap heads: uppercase eyebrow, the Geist
 * Pixel title printing in, an Inter lede in the muted colour.
 */
export function PageHead({ eyebrow, eyebrowHref, title, titleId, description, children, className }: PageHeadProps) {
  return (
    <header className={cn('max-w-[820px]', className)}>
      {eyebrow ? (
        <Reveal>
          {eyebrowHref ? (
            <a href={eyebrowHref} className={styles.eyebrow}>
              {eyebrow}
            </a>
          ) : (
            <p className={styles.eyebrow}>{eyebrow}</p>
          )}
        </Reveal>
      ) : null}
      <PixelHeading as="h1" id={titleId} className={styles.title}>
        {title}
      </PixelHeading>
      {description ? (
        <Reveal delay={0.1}>
          <p className={styles.lede}>{description}</p>
        </Reveal>
      ) : null}
      {children}
    </header>
  )
}
