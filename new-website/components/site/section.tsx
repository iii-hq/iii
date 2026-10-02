import { cn } from '@/lib/utils'
import { PixelHeading } from './pixel-heading'
import { Reveal } from './reveal'
import { SectionRule } from './section-rule'

type SectionProps = {
  id: string
  eyebrow: string
  title: React.ReactNode
  lede?: React.ReactNode
  className?: string
  /** Heading in a sticky left column with the content beside it (large screens). */
  split?: boolean
  children?: React.ReactNode
}

/** Section titles share the hero's Geist Pixel face. Pixel glyphs are wide, so sizes sit a step under Inter's. */
export const sectionTitleClass =
  'mt-4 text-[32px] leading-[1.1] tracking-[-0.01em] sm:mt-5 sm:text-[36px] md:text-[44px] md:leading-[1.08]'

/** Shared shell for every homepage section: hairline top rule, mono eyebrow, heading and lede. */
export function Section({ id, eyebrow, title, lede, className, split, children }: SectionProps) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className={cn('landing-section relative', className)}>
      <SectionRule />
      <div
        className={cn(
          'mx-auto max-w-[1240px] px-5 py-16 sm:px-6 sm:py-20 md:px-5 md:py-24',
          split && 'lg:grid lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-16',
        )}
      >
        <Reveal as="header" className={cn('max-w-3xl', split && 'lg:sticky lg:top-28 lg:self-start')}>
          <p className="font-medium font-sans text-muted-foreground text-xs uppercase leading-none tracking-[0.08em]">
            {eyebrow}
          </p>
          <PixelHeading id={`${id}-title`} className={sectionTitleClass}>
            {title}
          </PixelHeading>
          {lede ? (
            <div className="mt-4 max-w-[640px] text-pretty text-base text-muted-foreground leading-relaxed sm:text-[15px] md:text-[17px]">
              {lede}
            </div>
          ) : null}
        </Reveal>
        {children}
      </div>
    </section>
  )
}
