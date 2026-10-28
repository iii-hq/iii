import { ArrowUpRightIcon } from 'lucide-react'

import { Reveal } from '@/components/site/reveal'
import { buttonVariants } from '@/components/ui/button'
import data from '@/content/contributors/data.json'
import type { ContributorsData } from '@/lib/contributors'
import { cn } from '@/lib/utils'
import { contributors as copy } from './content'
import { ContributorGrid } from './contributor-grid'
import { Section } from './section'
import { StarButton } from './star-button'

const repoLink =
  'text-foreground underline decoration-line-strong underline-offset-4 transition-colors hover:decoration-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 rounded-sm'

/** A step under the closing CTA's buttons (h-11, 15px): this row supports the section, the close owns the page. */
const button = 'group h-10 w-full gap-2 rounded-[10px] px-4 text-[14px] sm:w-auto'

/**
 * Contributors: everyone who has landed a commit in a repository the site showcases, read from GitHub at build
 * time (scripts/generate-contributors.ts) with bots removed and people deduplicated across repositories. Sits
 * between the FAQ and the closing CTA: the last thing before "get started" is the people already building it.
 *
 * The two actions sit directly under the lede, in reading order, a size under the close's buttons: star the
 * engine (StarButton: the live count sits in the button, the same figure the site header shows) or open a pull
 * request.
 */
export function Contributors({ stars }: { stars: number | null }) {
  const { contributors, repos } = data as ContributorsData
  if (contributors.length === 0) return null
  const count = contributors.length.toLocaleString('en-US')
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section
      id="contributors"
      eyebrow={copy.eyebrow}
      title={copy.title}
      lede={
        <>
          <p>
            {count} people have landed commits in{' '}
            {repos.map((repo, i) => (
              <span key={repo}>
                {i > 0 ? (i === repos.length - 1 ? ' and ' : ', ') : ''}
                <a href={`https://github.com/${repo}`} target="_blank" rel="noopener noreferrer" className={repoLink}>
                  {repo}
                </a>
              </span>
            ))}
            . {copy.copy}
          </p>
          <div className="mt-6 flex flex-col gap-2.5 sm:flex-row sm:items-center">
            <StarButton href={copy.star.href} label={copy.star.label} stars={stars} className={button} />
            <a
              href={copy.pr.href}
              target="_blank"
              rel="noopener noreferrer"
              className={cn(
                buttonVariants({ variant: 'outline' }),
                button,
                'pr-3.5 text-foreground/90 hover:text-foreground dark:bg-transparent dark:hover:bg-faint',
              )}
            >
              {copy.pr.label}
              <ArrowUpRightIcon
                aria-hidden
                strokeWidth={1.75}
                className="size-4 text-muted-foreground transition-[translate,color] duration-150 group-hover:translate-x-px group-hover:-translate-y-px group-hover:text-foreground motion-reduce:transition-none"
              />
            </a>
          </div>
        </>
      }
    >
      <Reveal delay={0.1} className="mt-10 lg:mt-12">
        <ContributorGrid contributors={contributors} join={{ label: copy.you, href: copy.pr.href }} />
      </Reveal>
    </Section>
  )
}
