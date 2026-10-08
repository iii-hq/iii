/**
 * Copy for the landing page (Version A of the A/B test, formerly /a), lifted from "iii.dev - Landing Pages for A/B
 * Test" with the reviewer comments applied. Change words here, not inside the sections.
 */

export const hero = {
  /* The three-i glyph in the pill reads as the three primitives: everything in iii is one of these. */
  eyebrow: 'Three primitives: Function, Trigger, Worker',
  /* Reviewer: "iii isn't just backend" → "More application. Less infrastructure." */
  headline: ['More application.', 'Less infrastructure.'],
  /* 2026-10-05 sync: the doc's subtitle is folded into the copy, one paragraph under the headline. The hero sets the
     subtitle sentence in the foreground colour so the value line still carries weight. */
  copy: 'Connect any functions, agents, APIs, models, browsers, infrastructure, and machines into one live, composable system.',
  subtitle: 'The convenience of a platform, without the lock-in.',
  /* Anthony: the hero CTA sends people to the install page, not the quickstart. */
  cta: { label: 'Get started', href: 'https://iii.dev/docs/install' },
  secondary: { label: 'View GitHub', href: 'https://github.com/iii-hq/iii' },
  updates: 'Release notes and new workers, by email',
  /* The install block replaces the two CTA buttons. Commands and notes come from iii.dev/docs/install.
     2026-10-05 sync: no `workers` tab, it is internal jargon to a newcomer. */
  install: {
    tabs: [
      {
        id: 'curl',
        label: 'curl',
        steps: [
          {
            note: 'Install the engine. Press y at the prompt and the installer sets up the harness for you.',
            command: 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh',
          },
          { note: 'Then open http://127.0.0.1:3113' },
        ],
      },
      {
        id: 'no-llm',
        label: 'no llm',
        steps: [
          {
            note: 'Install the engine. Press n at the harness prompt.',
            command: 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh',
          },
          { note: 'Create a project', command: 'iii project init my-app && cd my-app' },
          { note: 'Start it, then open http://127.0.0.1:3113', command: 'iii compose --up' },
        ],
      },
      {
        id: 'non-interactive',
        label: 'ci',
        steps: [
          {
            note: 'Docker builds, CI jobs and scripts: the install only, no questions asked.',
            command: 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- --non-interactive',
          },
        ],
      },
    ],
    /* Left side of the CTA row; the card sits on the right. */
    title: 'Install iii in one command.',
    copy: 'The installer sets up the engine and, if you press y, the harness too. Then open http://127.0.0.1:3113.',
    guide: { label: 'Full install guide', href: 'https://iii.dev/docs/install' },
  },
  /* The hero trace: one request fanning through six spans. */
  spans: [
    { id: 'agent', label: 'Agent', fn: 'agent::run', start: 0, length: 100, kind: 'ai', depth: 0 },
    { id: 'queue', label: 'Queue', fn: 'queue::push', start: 6, length: 10, kind: 'q', depth: 1 },
    { id: 'db', label: 'Database Write', fn: 'pg::insert', start: 18, length: 16, kind: 'db', depth: 1 },
    { id: 'state', label: 'State React', fn: 'state::changed', start: 34, length: 8, kind: 'ev', depth: 1 },
    { id: 'test', label: 'Test', fn: 'ci::test', start: 44, length: 30, kind: 'ci', depth: 1 },
    { id: 'deploy', label: 'Deploy', fn: 'deploy::ship', start: 76, length: 22, kind: 'sh', depth: 1 },
  ],
} as const

export const overview = {
  eyebrow: 'Overview',
  title: "Code shouldn't care where code runs.",
  subtitle: 'One engine. Any workload.',
  /* Anthony (2026-10-05 sync): no problem / cost / solution labels, just three plain paragraphs. */
  paragraphs: [
    'Functions are stuck inside the language, process, cloud, and sometimes the machine they were written for.',
    'Connecting them means integrating discovery, invocation, retries, error handling, and tracing by hand, again and again, for every new boundary.',
    'Functions can live in different languages, processes, runtimes, and machines. iii makes them callable as if they were local.',
  ],
}

export const demo = {
  eyebrow: 'Live demo',
  title: 'Watch iii execute.',
  subtitle: 'Follow one request through the code, the workers, and the trace. Every call goes through iii.',
  prompt: 'Summarize the open PRs on iii-hq/workers.',
}

export const story = {
  eyebrow: 'C.O.D.E.R.',
  title: 'Build it up. See it work.',
  description: 'Start with HTTP and a database. Build, trace, and extend the same system as you scroll.',
  compose: {
    letter: 'C',
    name: 'Composability',
    title: 'Start small. Work together.',
    description:
      'Add HTTP and a database with Compose, then connect your own code. Functions in different languages work together through the same engine.',
  },
  observe: {
    letter: 'O',
    name: 'Observability',
    title: 'Every call tells its story.',
    description:
      'Follow one request across languages and machines. Trace context travels with each call, and logs stay attached to the span that wrote them.',
  },
  discover: {
    letter: 'D',
    name: 'Discoverability',
    title: 'Find what your system can do.',
    description:
      'Turn a request into the capabilities it needs. Search the running system for matching functions, and find workers in the registry when something is missing.',
  },
  extend: {
    letter: 'E',
    name: 'Extensibility',
    title: 'Add a capability. Grow the graph.',
    description:
      'Compose adds workers and resolves their dependencies. Storage, a model provider, or a complete harness joins the same system, ready for other workers to use.',
  },
  react: {
    letter: 'R',
    name: 'Reactivity',
    title: 'One event. Many reactions.',
    description:
      'Triggers make your system reactive. A pull request opens, and independent handlers review the code, notify the team, and run the tests.',
  },
  summary: {
    title: 'Built for C.O.D.E.R.',
    subtitle: 'Compose, observe, discover, extend, and react. All in the same system.',
    words: ['Composability', 'Observability', 'Discoverability', 'Extensibility', 'Reactivity'],
  },
}

export const proof = {
  eyebrow: 'Proof: Harness + ADE',
  title: 'A complete agent runtime, built on iii.',
  subtitle:
    'Harness and ADE combine models, tools, browsers, computers, sandboxes, state, and remote execution into one agent system, all running through the same iii graph.',
  solution:
    'A full agent runtime built with iii, inheriting every property above. Discovery is instant because every function is readily queried. Context stays small because every schema is already computed.',
  /* Reviewer: this CTA is the install page, since install brings users into the harness. */
  cta: { label: 'Explore the Harness', href: 'https://iii.dev/docs/install' },
}

export const useCases = {
  eyebrow: 'Use cases',
  title: 'Build anything.',
  subtitle: "iii isn't an agent framework or a workflow engine. It's the engine underneath both.",
  /* 2026-10-05 sync: no graph here, no problem subtitles, no links until the use case pages exist. "Agentic
     harness" became "Custom harness": we ship ours, and iii is how you build your own. App platform shows several
     workloads as a run log (Mike: data pipelining, training, "a bunch of stuff"). Infrastructure reproduces the
     compose terminal from iii-hq/iii#2263 spinning up the whole stack, as DOM lines rather than a video. */
  harness: {
    id: 'harness',
    label: 'Custom harness',
    title: 'Custom harness',
    solution:
      'Build an agent around your workflow. Choose models, tools, and session behavior while sharing the same functions as the rest of your system.',
    session: {
      label: 'Agent session',
      request: 'Research these pull requests and prepare a release summary.',
      steps: [
        'Find the available GitHub functions',
        'Read the open pull requests',
        'Ask your chosen model for a summary',
      ],
      result: 'A release summary, ready for your team.',
    },
  },
  platform: {
    id: 'platform',
    label: 'App platform',
    solution:
      'Build backends from functions running across languages, processes, and machines. A trigger starts the work, iii runs every call, and one trace follows it to the end.',
    picker: 'Workload',
    /* Trigger types and registry function ids checked against iii-hq/workers (http, cron, queue, database, storage,
       state, llm-router). Functions under your own namespace (orders::, etl::, reports::, train::, payments::) are
       the code you write. */
    workloads: [
      {
        id: 'api',
        label: 'API backend',
        detail: 'HTTP routes to functions in any language.',
        trigger: { type: 'http', detail: 'POST /orders' },
        calls: [
          { fn: 'orders::create', worker: 'api · Python', detail: 'your code', ms: 92 },
          { fn: 'database::execute', worker: 'database · Rust', detail: '1 row written', ms: 14 },
          { fn: 'state::set', worker: 'state · Rust', detail: 'order status', ms: 3 },
        ],
        result: 'Order created. One request, one trace.',
      },
      {
        id: 'pipeline',
        label: 'Data pipeline',
        detail: 'Durable queues feeding transforms and storage.',
        trigger: { type: 'durable:subscriber', detail: 'queue=ingest' },
        calls: [
          { fn: 'etl::transform', worker: 'etl · Python', detail: '2,400 records', ms: 318 },
          { fn: 'database::execute', worker: 'database · Rust', detail: '2,400 rows written', ms: 41 },
          { fn: 'storage::put', worker: 'storage · Rust', detail: 'batch archived', ms: 27 },
        ],
        result: 'Batch processed. Retries and dead letters handled by the queue.',
      },
      {
        id: 'scheduled',
        label: 'Scheduled jobs',
        detail: 'Cron expressions that call functions on time.',
        trigger: { type: 'cron', detail: '0 0 9 * * * *' },
        calls: [
          { fn: 'reports::daily-summary', worker: 'reports · TypeScript', detail: 'your code', ms: 48 },
          { fn: 'database::query', worker: 'database · Rust', detail: "yesterday's orders", ms: 22 },
          { fn: 'router::chat', worker: 'llm-router · Rust', detail: 'summary via provider-anthropic', ms: 910 },
          { fn: 'storage::put', worker: 'storage · Rust', detail: 'summary.md', ms: 19 },
        ],
        result: 'Daily summary written at 09:00. Every run traced.',
      },
      {
        id: 'training',
        label: 'Training job',
        detail: 'Long-running work on the machines that have the hardware.',
        trigger: { type: 'durable:subscriber', detail: 'queue=training' },
        calls: [
          { fn: 'train::epoch', worker: 'train · Python · GPU host', detail: 'epoch 12 of 50', ms: 184_000 },
          { fn: 'storage::put', worker: 'storage · Rust', detail: 'checkpoint-12.pt', ms: 2_140 },
          { fn: 'state::set', worker: 'state · Rust', detail: 'progress 24%', ms: 3 },
        ],
        result: 'Checkpoint saved. The next epoch is queued.',
      },
      {
        id: 'webhooks',
        label: 'Webhooks',
        detail: 'Receive an event, record it, and fan it out.',
        trigger: { type: 'http', detail: 'POST /webhooks/payments' },
        calls: [
          { fn: 'payments::record', worker: 'payments · TypeScript', detail: 'your code', ms: 31 },
          { fn: 'database::execute', worker: 'database · Rust', detail: 'payment stored', ms: 12 },
          { fn: 'iii::durable::publish', worker: 'queue · Rust', detail: 'queue=notify', ms: 4 },
        ],
        result: 'Payment recorded. Downstream consumers pick it up from the queue.',
      },
    ],
  },
  infra: {
    id: 'infra',
    label: 'Infrastructure',
    title: 'Infrastructure',
    solution:
      'Define workers in a Compose file and run them on infrastructure you control. One command starts the engine and the whole stack, dependencies included.',
    terminal: {
      label: 'iii compose',
      command: 'iii compose --up',
      engine: 'iii 0.24.6',
      /* Registry names from workers.iii.dev. `harness` pulls in its dependencies, shown indented under it. */
      workers: [
        { name: 'http', ms: 142 },
        { name: 'database', ms: 233 },
        { name: 'storage', ms: 198 },
        { name: 'queue', ms: 176 },
        { name: 'cron', ms: 121 },
        { name: 'state', ms: 109 },
        { name: 'harness', ms: 531 },
        { name: 'llm-router', ms: 264, dep: true },
        { name: 'provider-anthropic', ms: 212, dep: true },
        { name: 'provider-openai', ms: 205, dep: true },
        { name: 'context-manager', ms: 188, dep: true },
        { name: 'session-manager', ms: 173, dep: true },
        { name: 'judge', ms: 160, dep: true },
      ],
      done: 'open http://127.0.0.1:3113',
    },
  },
}

/**
 * Numbers. The engine benchmarks the team supplied on 2026-10-07 (Mike's figures, runner details from Gui).
 * Stars, contributors and workers already sit in Ownership directly above, so they are not repeated here.
 * Keep the same set on Version B.
 */
export const numbers = {
  eyebrow: 'Numbers',
  title: 'Built to execute.',
  metrics: [
    {
      id: 'invoke',
      value: '1.9',
      unit: 'µs',
      label: 'per function call',
      detail: '1 KB payload. 2.5 µs at 10 KB, 9.3 µs at 100 KB',
    },
    { id: 'concurrent', value: '2.8', unit: 'µs', label: 'per call, 128 at once', detail: '354 µs for all 128 calls' },
    {
      id: 'triggers',
      value: '3.9',
      unit: 'µs',
      label: 'per event, 128 triggers at once',
      detail: '500 µs for all 128 events',
    },
    { id: 'kv-read', value: '0.378', unit: 'µs', label: 'KV cache read', detail: '2.6M reads per second' },
    { id: 'kv-write', value: '0.653', unit: 'µs', label: 'KV cache write', detail: '1.5M writes per second' },
    { id: 'languages', value: '3', unit: null, label: 'SDK languages', detail: 'TypeScript, Python and Rust' },
  ],
  footnote: {
    setup:
      'Measured on a standard GitHub-hosted runner (ubuntu-latest: 4 CPU cores, 16 GB RAM, 14 GB SSD, x64), with the benchmarks using all 4 cores.',
    bench: 'We benchmark every change to iii as part of our performance analysis.',
    link: { label: 'View the benchmarks', href: 'https://iii-hq.github.io/iii/dev/bench/' },
  },
}

export const ownership = {
  eyebrow: 'Ownership',
  title: 'An architecture you own, not a vendor-locked platform.',
  subtitle: 'The convenience of a platform, without the lock-in.',
  copy: 'iii is an engine you run yourself: on your laptop, your cloud, or your hardware. Your code not only stays yours but can be trivially migrated anywhere. Any worker can be swapped out, forked, or replaced. When pricing changes or a vendor changes direction, your architecture still belongs to you.',
  /* Where each environment runs, in the copy's own words ("your laptop, your cloud, or your hardware"). */
  environments: [
    { name: 'Local', where: 'your laptop' },
    { name: 'Self-hosted', where: 'your hardware' },
    { name: 'Cloud', where: 'your cloud' },
  ],
  /* The app that moves between them: the engine plus registry workers (names from workers.iii.dev). */
  app: { engine: 'iii engine', workers: ['http', 'database', 'state', 'harness'] },
  /*
   * Proof that people build on it, in the order the 2026-10-05 sync set: workers first, then contributors,
   * then downloads. GitHub stars were dropped here ("not relevant to this particular detail").
   * Every figure is read live from `source` and links to the public page that shows the same number, so a
   * reader can check it themselves.
   */
  stats: [
    { id: 'workers', label: 'Workers in the registry', source: 'workers.iii.dev', href: 'https://workers.iii.dev/' },
    {
      id: 'contributors',
      label: 'Contributors',
      source: 'GitHub',
      href: 'https://github.com/iii-hq/iii/graphs/contributors',
    },
    { id: 'npmWeek', label: 'npm downloads a week', source: 'npm', href: 'https://www.npmjs.com/package/iii-sdk' },
    {
      id: 'pypiWeek',
      label: 'PyPI downloads a week',
      source: 'pypistats.org',
      href: 'https://pypistats.org/packages/iii-sdk',
    },
    {
      id: 'crates90d',
      label: 'crates.io downloads in 90 days',
      source: 'crates.io',
      href: 'https://crates.io/crates/iii-sdk',
    },
    { id: 'dockerPulls', label: 'Docker Hub pulls', source: 'Docker Hub', href: 'https://hub.docker.com/r/iiidev/iii' },
  ],
}

export const finalCta = {
  title: 'Make anything callable.',
  copy: 'Start building your iii system.',
  primary: { label: 'Get started', href: 'https://iii.dev/docs/install' },
  secondary: { label: 'View GitHub', href: 'https://github.com/iii-hq/iii' },
}
