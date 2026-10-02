import type { BundledLanguage } from 'shiki'

/** The agent run behind the live demo. Line indices are referenced by `HOPS[].code` in graph/model.ts. */
export const demoCode = {
  file: 'harness.ts',
  lang: 'ts' as BundledLanguage,
  lines: [
    'const call = (function_id, payload) => iii.trigger({ function_id, payload })',
    '',
    "iii.registerFunction('agent::run', async ({ prompt }) => {",
    '  const plan = await harness.plan(prompt)',
    '  for (const pr of plan.targets) {',
    "    const page = await call('browser::navigate', { url: pr.url })",
    "    const html = await call('browser::act', { page, action: 'open files' })",
    "    const text = await call('extract::text', { html })",
    "    const vector = await call('embed::vectors', { text })",
    "    const similar = await call('pg::query', { sql: NEAREST, params: [vector] })",
    "    const summary = await call('llm::complete', { prompt: digest(text, similar) })",
    "    await call('github::pr::watch', { number: pr.number, on: 'merged' })",
    '  }',
    '})',
  ],
}

/** Log lines shown in the node detail panel, per function. */
export const demoLogs: Record<string, string[]> = {
  'agent::run': ['plan: 3 open PRs on iii-hq/workers', 'step 1/8 → browser::navigate'],
  'browser::navigate': ['GET github.com/iii-hq/workers/pulls', 'page #p-91 ready · 412ms'],
  'browser::act': ['click "Files changed"', '14 files · diff loaded'],
  'extract::text': ['html → text · 48 KB in', '9ms · 6.1 KB out'],
  'embed::vectors': ['cuda:0 · batch 1', '1536 dims · 61ms'],
  'pg::query': ['select … order by embedding <-> $1 limit 4', '4 rows · 14ms'],
  'llm::complete': ['model: local/qwen-72b', '318 tokens · 920ms'],
  'github::pr::watch': ['subscribed: pr #418 merged', 'trigger registered on github worker'],
}
