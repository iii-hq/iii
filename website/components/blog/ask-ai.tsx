import { assistantIcons } from '@/components/icons/assistants'
import { askAssistants } from '@/lib/site'

/**
 * "Understand this post with AI": one square per assistant, logo only, opening it on a question that points at the
 * post's markdown export. The name shows as a tooltip on hover and keyboard focus (CSS only, so it works before
 * hydration) and is the link's accessible name.
 */
export function AskAi({ markdownUrl }: { markdownUrl: string }) {
  const assistants = askAssistants(
    `Read ${markdownUrl} and help me understand it: summarize the main points, explain the key terms, and then answer my follow-up questions.`,
  )

  return (
    <div className="mt-6 flex flex-wrap items-center gap-x-4 gap-y-3">
      <p className="text-[14px] text-muted-foreground">Understand this post with AI</p>
      <ul className="flex items-center gap-2">
        {assistants.map((a) => {
          const Icon = assistantIcons[a.id]
          return (
            <li key={a.id} className="relative">
              <a
                href={a.href}
                target="_blank"
                rel="noopener noreferrer"
                aria-label={`Ask ${a.label} about this post (opens in a new tab)`}
                className="peer flex size-9 items-center justify-center rounded-lg border text-muted-foreground outline-none transition-colors hover:border-line-strong hover:text-foreground focus-visible:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
              >
                <Icon className="size-4" />
              </a>
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
