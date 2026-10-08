import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { highlight } from '@/lib/highlight'
import { OneFunctionDemo, type OneFunctionSnippet } from './one-function-demo'

const snippets: Omit<OneFunctionSnippet, 'html'>[] = [
  {
    path: 'http',
    file: 'route.ts',
    lang: 'ts',
    via: 'HTTP',
    title: 'The route is a function too.',
    description:
      'It receives the HTTP request and returns the response, so it decides what the outside world sees: validation, status codes, which fields to expose.',
    code: `// The logic: plain input, plain output.
iii.registerFunction('reports::generate', async ({ team }: { team: string }) => {
  return buildReport(team)
})

// The route is a function too: it receives the HTTP request and returns the response.
iii.registerFunction('api::reports::generate', async (req: { body: { team: string } }) => {
  const report = await iii.trigger({
    function_id: 'reports::generate',
    payload: { team: req.body.team },
  })
  return { status_code: 200, body: report }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'api::reports::generate',
  config: { api_path: '/reports/generate', http_method: 'POST' },
})`,
  },
  {
    path: 'python',
    file: 'caller.py',
    lang: 'python',
    via: 'Python',
    title: 'The Python worker calls it by name.',
    description: 'Any worker calls it by name, without knowing where it runs.',
    code: `report = iii.trigger({
    "function_id": "reports::generate",
    "payload": {"team": "billing"},
})`,
  },
  {
    path: 'queue',
    file: 'enqueue.ts',
    lang: 'ts',
    via: 'Queue',
    title: 'The queue runs it in the background.',
    description:
      'Failed messages retry with exponential backoff; messages that keep failing move to a dead-letter queue you can retry or discard.',
    code: `import { TriggerAction } from 'iii-sdk'

await iii.trigger({
  function_id: 'reports::generate',
  payload: { team: 'billing' },
  action: TriggerAction.Enqueue({
    queue: 'reports',
  }),
})`,
  },
]

export async function OneFunction() {
  const highlighted = await Promise.all(
    snippets.map(async (snippet) => ({ ...snippet, html: await highlight(snippet.code, snippet.lang) })),
  )
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: The homepage has one stable anchor for this section.
    <Section
      id="one-function"
      eyebrow="One function, many entry points"
      title="Write the logic once. Reach it from anywhere."
      lede={
        <>
          The logic lives in <code className="font-mono text-[0.9em] text-foreground">reports::generate</code>. The
          Python worker calls it by name, the queue runs it in the background, and the HTTP route calls it the same way.
        </>
      }
    >
      <Reveal delay={0.1}>
        <OneFunctionDemo snippets={highlighted} />
      </Reveal>
    </Section>
  )
}
