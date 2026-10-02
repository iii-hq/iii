import { ArrowRightIcon, ArrowUpRightIcon } from 'lucide-react'

import { HeroEyebrow } from '@/components/graphics/hero-eyebrow'
import { HeroRuntime } from '@/components/graphics/hero-runtime'
import { heroScenes } from '@/components/graphics/hero-scenes'
import { HeroTitle } from '@/components/graphics/hero-title'
import { Reveal } from '@/components/site/reveal'
import { buttonVariants } from '@/components/ui/button'
import { tokenize } from '@/lib/highlight'
import { links } from '@/lib/site'
import { cn } from '@/lib/utils'

const ctaBase = 'group h-12 w-full gap-2.5 rounded-xl px-5 text-[15px] sm:h-11 sm:w-auto'

export async function Hero() {
  const code = await Promise.all(heroScenes.map((scene) => tokenize(scene.code.join('\n'), scene.lang)))
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: The homepage has one hero and this is its public anchor.
    <section id="hero" aria-labelledby="hero-title" className="relative overflow-hidden">
      <div className="mx-auto flex w-full max-w-[1240px] flex-col items-center px-5 pt-24 pb-14 sm:px-6 sm:pt-28 sm:pb-16 md:px-5 lg:pt-36 lg:pb-24">
        <div className="flex w-full max-w-[760px] flex-col items-center text-center">
          <Reveal className="flex w-full min-w-0 justify-center">
            <HeroEyebrow />
          </Reveal>
          <Reveal delay={0.05} className="mt-5 w-full sm:mt-6">
            <HeroTitle />
          </Reveal>
          <Reveal delay={0.1} className="mt-6 sm:mt-7">
            <p className="mx-auto max-w-[640px] text-pretty font-sans text-base text-muted-foreground leading-[1.55] sm:text-[15px] sm:leading-[1.6] md:text-[17px]">
              One place to run your APIs, background jobs, schedules, and AI agents, in any language.
            </p>
          </Reveal>
          <Reveal
            delay={0.15}
            className="mt-8 flex w-full max-w-sm flex-col items-stretch gap-3 sm:mt-9 sm:w-auto sm:max-w-none sm:flex-row sm:flex-wrap sm:items-center sm:justify-center"
          >
            <a href={links.quickstart} className={cn(buttonVariants(), ctaBase, 'pr-4')}>
              Run the quickstart
              <ArrowRightIcon
                aria-hidden
                strokeWidth={1.75}
                className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
              />
            </a>
            <a
              href={links.registry}
              className={cn(
                buttonVariants({ variant: 'outline' }),
                ctaBase,
                'pr-4 text-foreground/90 hover:text-foreground dark:bg-transparent dark:hover:bg-faint',
              )}
            >
              Browse the registry
              <ArrowUpRightIcon
                aria-hidden
                strokeWidth={1.75}
                className="size-4 text-muted-foreground transition-[translate,color] duration-150 group-hover:translate-x-px group-hover:-translate-y-px group-hover:text-foreground motion-reduce:transition-none"
              />
            </a>
          </Reveal>
        </div>
        <Reveal delay={0.2} className="mt-12 w-full sm:mt-14 lg:mt-16">
          <HeroRuntime code={code} />
        </Reveal>
      </div>
    </section>
  )
}
