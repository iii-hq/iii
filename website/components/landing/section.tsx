import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { SectionRule } from '@/components/site/section-rule'
import { cn } from '@/lib/utils'

export { sectionTitleClass } from '@/components/site/section'

/** The landing runs on a wide container: 90% of the viewport, capped on very large screens. */
export const wideContainer = 'mx-auto w-[90%] max-w-[1720px]'

type SectionProps = {
  id: string
  eyebrow: string
  title: React.ReactNode
  lede?: React.ReactNode
  className?: string
  children?: React.ReactNode
}

/** Same anatomy as the homepage section (rule, mono eyebrow, pixel heading, lede) on the wide container. */
export function Section({ id, eyebrow, title, lede, className, children }: SectionProps) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className={cn('landing-section relative', className)}>
      <SectionRule />
      <div className={cn(wideContainer, 'py-16 sm:py-20 md:py-24')}>
        <Reveal as="header" className="max-w-3xl">
          <p className="font-medium font-sans text-muted-foreground text-xs uppercase leading-none tracking-[0.08em]">
            {eyebrow}
          </p>
          <PixelHeading
            id={`${id}-title`}
            className="mt-4 text-[32px] leading-[1.1] tracking-[-0.01em] sm:mt-5 sm:text-[36px] md:text-[44px] md:leading-[1.08]"
          >
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
