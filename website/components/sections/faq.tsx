import type { ReactNode } from 'react'
import {
  Accordion,
  AccordionItem,
  AccordionPanel,
  AccordionTrigger,
} from '@/components/animate-ui/components/base/accordion'

import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'

/** Exported so the page can publish the same questions as FAQPage structured data. */
export const faqs: { q: string; a: string }[] = [
  {
    q: 'What is iii?',
    a: 'An engine that programs connect to over WebSocket. Each registers functions and triggers; the engine keeps a live registry of them, routes every call, and traces it.',
  },
  {
    q: 'How is iii different from Claude Code, Codex, or Cursor?',
    a: 'Those are coding agents; iii is the system they can run in. Install them as workers and keep using them. iii adds what surrounds them: your functions as their tools, permissions and approvals, durable queues, triggers that wake them, and traces shared with the rest of the system.',
  },
  {
    q: 'How is iii different from MCP?',
    a: 'MCP describes tools to a client. In iii, tools are the functions your system already runs, and every worker can call them, not only agents. The `mcp` worker exposes the functions you choose to any MCP client.',
  },
  {
    q: 'Do I need a database?',
    a: 'No. Storage is optional workers: `state` for key-value (built-in store or Redis), `database` for PostgreSQL, MySQL, or SQLite, and `storage` for S3-compatible objects. Each exposes functions and emits change triggers. Install what you need.',
  },
  {
    q: 'Do I need AI?',
    a: 'No. The harness is an optional worker.',
  },
  {
    q: 'Do I have to rewrite my app?',
    a: 'No. Wrap one service or script as a worker and leave the rest as it is. Route functions on the `http` worker can answer at the paths your clients already call, so the clients don’t change.',
  },
  {
    q: 'Which languages can I use?',
    a: 'SDKs for TypeScript/Node, Python, Rust, Go, and browser JavaScript. Any other language can implement the WebSocket protocol. Mix them in one system.',
  },
  {
    q: 'How do I control what agents can do?',
    a: 'Agents start with access to nothing. You allow function ids or globs per session, require human approval for chosen calls, filter calls in hooks, run untrusted code in isolates or microVMs, and separate teams with namespaces and an RBAC proxy.',
  },
  {
    q: 'Is it ready for production?',
    a: 'iii is pre-1.0, and APIs can still change. Compose pins every worker’s version, so your setup changes only when you update it. Worker releases are immutable and checksummed.',
  },
  {
    q: 'What is the licensing?',
    a: 'The engine is source-available under the Elastic License 2.0. The SDKs and the workers in `iii-hq/workers` are Apache 2.0.',
  },
]

/** Renders `code` spans in otherwise plain answer text. */
function withCode(text: string) {
  return text.split(/(`[^`]+`)/g).map((part, i) =>
    part.startsWith('`') ? (
      // biome-ignore lint/suspicious/noArrayIndexKey: static split of a constant string
      <code key={i} className="rounded border bg-muted px-1 py-px font-mono text-[0.85em] text-foreground">
        {part.slice(1, -1)}
      </code>
    ) : (
      part
    ),
  )
}

/** `container` lets a page with its own width line the section up; `lede` sits under the heading. */
export function Faq({ container, lede }: { container?: string; lede?: ReactNode } = {}) {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section id="faq" eyebrow="Questions" title="Questions." lede={lede} container={container} split>
      <Reveal delay={0.1} className="mt-12 lg:mt-0">
        <Accordion className="border-t">
          {faqs.map((item) => (
            <AccordionItem key={item.q} value={item.q}>
              <AccordionTrigger className="py-5 font-medium text-[15px] text-foreground/90 hover:text-foreground hover:no-underline md:text-base [&>svg]:mt-0.5">
                {item.q}
              </AccordionTrigger>
              <AccordionPanel className="pb-6 text-[15px] text-muted-foreground leading-relaxed">
                {withCode(item.a)}
              </AccordionPanel>
            </AccordionItem>
          ))}
        </Accordion>
      </Reveal>
    </Section>
  )
}
