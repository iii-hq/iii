import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { CoderExplorer, type CoderItem } from './coder-explorer'

const items: CoderItem[] = [
  {
    id: 'composability',
    letter: 'C',
    name: 'Composability',
    short: 'Compose',
    hint: 'Anything you build joins the system',
    claim: 'Anything you build becomes part of the system.',
    body: 'Once a worker connects, every other worker, agents included, can use it. compose::add installs it as a running service with a pinned version.',
    example: 'worker connects → functions registered → ready to call',
  },
  {
    id: 'observability',
    letter: 'O',
    name: 'Observability',
    short: 'Observe',
    hint: 'The engine traces every call',
    claim: 'The engine traces every call.',
    body: 'Trace context follows every trigger() across languages, queues, and sub-agents, and logs attach to the span that wrote them. You don’t instrument each worker.',
    example: 'agent → function → queue → Python worker',
  },
  {
    id: 'discoverability',
    letter: 'D',
    name: 'Discoverability',
    short: 'Discover',
    hint: 'The system describes itself',
    claim: 'The system describes itself, to people and to models.',
    body: 'An iii agent lists what exists, reads a contract, and calls it from inside the system. In iii, the registry is the tool list.',
    example: 'discover → read the contract → call the function',
  },
  {
    id: 'extensibility',
    letter: 'E',
    name: 'Extensibility',
    short: 'Extend',
    hint: 'Workers extend the running system',
    claim: 'Workers extend the running system, interface included.',
    body: 'Add a worker while everything runs, with no redeploy or restart. Workers can also ship UI into the iii ADE, hot-reloaded.',
    example: 'new worker → new functions + UI → same running system',
  },
  {
    id: 'reactivity',
    letter: 'R',
    name: 'Reactivity',
    short: 'React',
    hint: 'Any change can start work',
    claim: 'Any change can start work.',
    body: 'Bind any function to any trigger, in any language, with conditions. An agent waiting on a ticket reply wakes up when it happens instead of polling.',
    example: 'event → trigger → function',
  },
]

export function Coder() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: The homepage has one stable anchor for this section.
    <Section
      id="coder"
      eyebrow="CODER"
      title="Five properties of every iii system."
      lede="They come from one design choice: every call passes through an engine that knows every function."
    >
      <Reveal delay={0.1}>
        <CoderExplorer items={items} />
      </Reveal>
    </Section>
  )
}
