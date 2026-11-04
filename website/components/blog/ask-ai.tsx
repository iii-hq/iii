import { assistantIcons } from '@/components/icons/assistants'
import { TrackedLink } from '@/components/site/tracked-link'
import { site } from '@/lib/seo'
import { askAssistants } from '@/lib/site'
import { cn } from '@/lib/utils'

type AskAiProps = {
  slug: string
  /** `top` sits under the meta line for skimmers; `end` follows the body for readers with follow-up questions */
  position: 'top' | 'end'
}

const copy = {
  top: 'Understand this post with AI',
  end: 'Questions about this post? Ask an AI',
} as const

/**
 * One square per assistant, logo only, opening it on a question that points at the post's markdown export. The name
 * shows as a tooltip on hover and keyboard focus (CSS only, so it works before hydration) and is the link's
 * accessible name. Each click sends `ask_ai_click` with the assistant, position and post, so the two placements can
 * be compared.
 */
export function AskAi({ slug, position }: AskAiProps) {
  const assistants = askAssistants(
    `Read ${site.url}/blog/${slug}.md and help me understand it: summarize the main points, explain the key terms, and then answer my follow-up questions.`,
  )

  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-x-4 gap-y-3',
        position === 'top' ? 'mt-6' : 'mt-16 justify-between rounded-xl border px-5 py-4',
      )}
    >
      <p className={cn('text-[14px]', position === 'top' ? 'text-muted-foreground' : 'text-foreground')}>
        {copy[position]}
      </p>
      <ul className="flex items-center gap-2">
        {assistants.map((a) => {
          const Icon = assistantIcons[a.id]
          return (
            <li key={a.id} className="relative">
              <TrackedLink
                href={a.href}
                target="_blank"
                rel="noopener noreferrer"
                aria-label={`Ask ${a.label} about this post (opens in a new tab)`}
                event="ask_ai_click"
                params={{ assistant: a.id, position, post_slug: slug }}
                className="peer flex size-9 items-center justify-center rounded-lg border text-muted-foreground outline-none transition-colors hover:border-line-strong hover:text-foreground focus-visible:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
              >
                <Icon className="size-4" />
              </TrackedLink>
              <span
                aria-hidden="true"
                className="pointer-events-none absolute bottom-full left-1/2 mb-2 -translate-x-1/2 translate-y-1 whitespace-nowrap rounded-md bg-foreground px-2 py-1 font-medium text-[12px] text-background opacity-0 transition-[opacity,translate] duration-150 peer-hover:translate-y-0 peer-hover:opacity-100 peer-focus-visible:translate-y-0 peer-focus-visible:opacity-100 motion-reduce:transition-none"
              >
                {a.label}
              </span>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
