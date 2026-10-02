import { ArrowRightIcon, ArrowUpRightIcon } from 'lucide-react'

import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { SectionRule } from '@/components/site/section-rule'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'

import { finalCta } from './content'
import { wideContainer } from './section'

const ctaBase = 'group h-12 w-full gap-2.5 rounded-xl px-5 text-[15px] sm:h-11 sm:w-auto'

/**
 * The closing CTA as one row on the wide container: the message on the left, the actions on the right, both
 * sitting on the same baseline so the eye reads title, line, button in one pass. Stacks on phones.
 */
export function FinalCta() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: Stable anchors for the single closing CTA.
    <section id="final-cta" aria-labelledby="final-cta-title" className="relative">
      <SectionRule />
      <div
        className={cn(
          wideContainer,
          'flex flex-col gap-8 py-20 md:py-28 lg:flex-row lg:items-center lg:justify-between lg:gap-16',
        )}
      >
        <Reveal className="flex min-w-0 flex-col">
          {/* biome-ignore lint/correctness/useUniqueElementIds: The single closing CTA owns this heading anchor. */}
          <PixelHeading
            id="final-cta-title"
            className="text-[32px] leading-[1.1] tracking-[-0.01em] sm:text-[36px] md:text-[44px] md:leading-[1.08]"
          >
            {finalCta.title}
          </PixelHeading>
          <p className="mt-3 max-w-[520px] text-pretty font-sans text-base text-muted-foreground leading-relaxed sm:mt-4 sm:text-[17px]">
            {finalCta.copy}
          </p>
        </Reveal>
        <Reveal
          delay={0.1}
          className="flex w-full max-w-sm flex-col items-stretch gap-3 sm:w-auto sm:max-w-none sm:flex-row sm:items-center lg:shrink-0 lg:justify-end"
        >
          <a href={finalCta.primary.href} className={cn(buttonVariants(), ctaBase, 'pr-4')}>
            {finalCta.primary.label}
            <ArrowRightIcon
              aria-hidden
              strokeWidth={1.75}
              className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
            />
          </a>
          <a
            href={finalCta.secondary.href}
            className={cn(
              buttonVariants({ variant: 'outline' }),
              ctaBase,
              'pr-4 text-foreground/90 hover:text-foreground dark:bg-transparent dark:hover:bg-faint',
            )}
          >
            {finalCta.secondary.label}
            <ArrowUpRightIcon
              aria-hidden
              strokeWidth={1.75}
              className="size-4 text-muted-foreground transition-[translate,color] duration-150 group-hover:translate-x-px group-hover:-translate-y-px group-hover:text-foreground motion-reduce:transition-none"
            />
          </a>
        </Reveal>
      </div>
    </section>
  )
}
