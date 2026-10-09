import { EmailSignup } from '@/components/site/email-signup'
import { PixelHeading } from '@/components/site/pixel-heading'
import { Reveal } from '@/components/site/reveal'
import { tokenize } from '@/lib/highlight'

import { hero } from './content'
import { InstallCommand, type InstallTab } from './install-command'

/** The install paths from the content file, each command tokenized once on the server for the card. */
async function installTabs(): Promise<InstallTab[]> {
  return Promise.all(
    hero.install.tabs.map(async (tab) => {
      const commands = tab.steps.flatMap((step) => ('command' in step ? [step.command] : []))
      const tokens = await Promise.all(commands.map((command) => tokenize(command, 'bash')))
      return {
        id: tab.id,
        label: tab.label,
        commands: commands.map((command, i) => ({ command, tokens: tokens[i].flat() })),
      }
    }),
  )
}

export async function Hero() {
  const tabs = await installTabs()
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: The page has one hero and this is its public anchor.
    <section id="hero" aria-labelledby="hero-title" className="relative overflow-hidden">
      <div className="mx-auto flex w-full max-w-[1240px] flex-col items-center px-5 pt-32 pb-14 sm:px-6 sm:pt-36 sm:pb-16 md:px-5 lg:pt-44 lg:pb-24">
        {/* A 680px column centred on the page (840px on desktop, room for the headline): eyebrow, headline,
            paragraph and install card all share one centre line. The install card and the email row stay 680px
            everywhere (2026-10-09: narrower on desktop); the `ci` command wraps onto a second line there. */}
        <div className="flex w-full max-w-[680px] flex-col items-center text-center lg:max-w-[840px]">
          <Reveal className="flex w-full min-w-0 justify-center">
            <span className="inline-flex h-7 items-center gap-2 rounded-full border px-3 font-sans text-[13px] text-foreground/80 leading-none sm:h-8 sm:text-[13px]">
              <span aria-hidden className="flex items-end gap-px">
                {[0, 1, 2].map((i) => (
                  <span key={i} className="flex flex-col items-center gap-px">
                    <span className="size-[3px] bg-foreground" />
                    <span className="h-2 w-[3px] bg-foreground" />
                  </span>
                ))}
              </span>
              {hero.eyebrow}
            </span>
          </Reveal>
          <Reveal delay={0.05} className="mt-5 w-full sm:mt-6">
            {/* biome-ignore lint/correctness/useUniqueElementIds: The page has one primary heading. */}
            <PixelHeading
              as="h1"
              id="hero-title"
              delay={0.15}
              className="text-center text-[clamp(1.75rem,9.4vw,2.75rem)] leading-[1.06] tracking-[-0.02em] sm:text-[clamp(2.25rem,5vw,4.25rem)] sm:leading-[1.04]"
            >
              {hero.headline.map((line) => (
                <span key={line} className="block whitespace-nowrap">
                  {line}
                </span>
              ))}
            </PixelHeading>
          </Reveal>
          <Reveal delay={0.1} className="mt-6 w-full sm:mt-7">
            {/* One paragraph, two tones: the description in muted, the doc's subtitle in the foreground so the value
                line still reads as the point. 16px at every size; the description runs on from the subtitle on the same line. */}
            <p className="mx-auto max-w-[560px] text-pretty text-center text-[16px] text-muted-foreground leading-[1.6] tracking-[-0.005em]">
              <span className="text-foreground">{hero.subtitle}</span> {hero.copy}
            </p>
          </Reveal>
          {/* The install block is the hero's only call to action: one command, nothing to click through. */}
          <Reveal delay={0.15} className="mt-8 flex w-full max-w-[680px] sm:mt-9">
            <InstallCommand tabs={tabs} />
          </Reveal>
        </div>
        {/* Anthony: an email capture near the top, kept quiet so it doesn't compete with the install block. */}
        <Reveal
          delay={0.2}
          className="mt-6 flex w-full max-w-[680px] flex-col items-start gap-3 px-1 sm:flex-row sm:items-center sm:justify-between"
        >
          <p className="font-sans text-[13px] text-muted-foreground">{hero.updates}</p>
          <EmailSignup className="sm:max-w-[340px]" />
        </Reveal>
      </div>
    </section>
  )
}
