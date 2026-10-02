/**
 * Copy for the Version A landing page (/a), lifted from "iii.dev - Landing Pages for A/B Test" with the reviewer
 * comments applied. Change words here, not inside the sections.
 */

export const hero = {
  pronounced: 'iii, pronounced "three i"',
  /* Reviewer: "iii isn't just backend" → "More application. Less infrastructure." */
  headline: ['More application.', 'Less infrastructure.'],
  copy: 'Connect any functions, agents, APIs, models, browsers, infrastructure, and machines into one live, composable system.',
  /* Anthony: the hero CTA sends people to the install page, not the quickstart. */
  cta: { label: 'Get started', href: 'https://iii.dev/docs/install' },
  secondary: { label: 'View GitHub', href: 'https://github.com/iii-hq/iii' },
  updates: 'Release notes and new workers, by email',
  /* The install block replaces the two CTA buttons. Commands and notes come from iii.dev/docs/install. */
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
      {
        id: 'workers',
        label: 'workers',
        steps: [
          {
            note: 'Scaffold a harness project and add workers from the registry in one go.',
            command: 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- --start-with database,storage',
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
  problem: 'Functions are stuck inside the language, process, cloud, and sometimes the machine they were written for.',
  agitation:
    'Connecting them means integrating discovery, invocation, retries, error handling, and tracing by hand, again and again, for every new boundary.',
  solution:
    'Functions can live in different languages, processes, runtimes, and machines. iii makes them callable as if they were local.',
  /* Reviewer: show the graph being fed from the registry too: an agent or a human installs or writes a worker. */
  pulls: [
    { who: 'agent', verb: 'installs from the registry', what: 'browser@1.8.0' },
    { who: 'agent', verb: 'writes its own worker', what: 'extract::text' },
    { who: 'human', verb: 'installs from the registry', what: 'postgres@3.0.4' },
  ],
}

export const demo = {
  eyebrow: 'Live demo',
  title: 'Watch iii execute.',
  subtitle: 'One request. An entire system comes alive.',
  solution: 'This is the moment it clicks: iii is the thing connecting and executing all of this.',
  prompt: 'Review the open PRs on iii-hq/workers and post a digest to #releases',
  views: ['Graph', 'Code', 'Trace'] as const,
}

export const story = {
  eyebrow: 'C.O.D.E.R.',
  compose: {
    letter: 'C',
    name: 'Composability',
    title: 'Use the right language for the job. Together.',
    subtitle: 'Everything can work with everything else.',
    tagline: 'No integration cruft.',
    solution:
      "iii doesn't force your whole system into one runtime. Functions written in different languages compose into the same execution graph.",
    rows: [
      ['TypeScript', 'Python'],
      ['Python', 'Rust'],
      ['Rust', 'Browser'],
      ['Browser', 'GPU'],
      ['GPU', 'Database'],
      ['Database', 'Agent'],
    ],
    /* Anthony: compose covers registry workers and the ones you write, then the functions between them. */
    files: [
      {
        name: 'worker-compose-local.yaml',
        lang: 'yaml' as const,
        code: [
          'workers:',
          '  github:   { from: registry, version: 1.4.2 }',
          '  browser:  { from: registry, version: 1.8.0 }',
          '  postgres: { from: registry, version: 3.0.4 }',
          '  extract:  { path: ./workers/extract }      # yours',
        ],
      },
      {
        name: 'worker-compose-gpu.yaml',
        lang: 'yaml' as const,
        code: [
          'workers:',
          '  llm:   { from: registry, version: 2.3.0, device: cuda }',
          '  embed: { path: ./workers/embed, device: cuda }  # yours',
        ],
      },
    ],
    chain: ['browser::act', 'extract::text', 'embed::vectors', 'pg::query'],
    /* The same chain as code: four workers, three languages, one call site each. */
    chainCode: [
      "const html = await call('browser::act', { page })",
      "const text = await call('extract::text', { html })",
      "const vector = await call('embed::vectors', { text })",
      "await call('pg::query', { sql: NEAREST, params: [vector] })",
    ],
  },
  observe: {
    letter: 'O',
    name: 'Observability',
    title: 'Every function is observable.',
    subtitle: 'Execution is traced end to end, across languages and machines.',
    tagline: 'One trace. Every hop.',
    solution:
      'Trace context follows every call across languages, queues, and machines. Logs attach to the span that wrote them. Nothing is instrumented by hand.',
    /* Anthony: show the trace leaving the system too, over OTLP, not written to a summary. */
    export: 'OTLP export · your observability stack',
  },
  extend: {
    letter: 'E',
    name: 'Extensibility',
    title: 'Everything is extensible.',
    subtitle: 'Give iii a capability. It joins the graph.',
    tagline: 'Anything can become a Worker.',
    solution: 'Workers expose capabilities, and iii makes those capabilities callable from anywhere.',
    handoff:
      'Nothing formatted a digest, so the harness wrote digest::format. It joined through the same Worker SDK as everything else.',
    categories: [
      { label: 'AI', items: ['OpenAI', 'Anthropic', 'Local models', 'Embedding', 'Judge'] },
      { label: 'Compute', items: ['Sandbox', 'Docker', 'Kubernetes', 'Shell', 'GPU'] },
      { label: 'Interaction', items: ['Browser', 'Computer', 'Filesystem'] },
      { label: 'Data', items: ['Postgres', 'Redis', 'State', 'Cache', 'Vector DB'] },
      { label: 'Connectivity', items: ['HTTP', 'WebSocket', 'Tailscale'] },
      { label: 'Automation', items: ['Queue', 'Scheduler', 'Events'] },
      { label: 'Developer', items: ['GitHub', 'Git', 'CI/CD'] },
    ],
    build: {
      title: 'Build your own',
      body: 'Anything with code becomes a service. Write a worker in any language and it joins the library.',
    },
    code: [
      'const page = await iii.browser.navigate(url)',
      'const result = await iii.python.analyze(page)',
      'await iii.database.store(result)',
    ],
  },
  discover: {
    letter: 'D',
    name: 'Discoverability',
    title: "Your system can change while it's running.",
    subtitle: 'Capabilities discover each other at runtime.',
    tagline: 'The graph is alive.',
    solution:
      'Workers can join and leave at any time. Their capabilities are discovered in real time, and the graph updates without service disruption.',
    /* Anthony: discovery takes the whole prompt, works out what satisfies it, then checks three levels in order:
       the running system's own registry, workers.iii.dev, and finally writing a worker (which hands off to Extensibility). */
    needs: ['github', 'browser', 'summarize', 'slack', 'digest'],
    before: 'An agent asks "what can I do?" and gets a stale spec.',
    after: 'A worker connects, its functions appear in the live list, and the agent uses them right away.',
  },
  react: {
    letter: 'R',
    name: 'Reactivity',
    title: "Don't just call functions. React to them.",
    subtitle: 'Calling says "do this." Observing says "when this happens, do this."',
    tagline: "Observation isn't just telemetry. It's composition.",
    before:
      'github::pr::watch calls digest::format directly. Adding slack::post and a review step means editing pr::watch again.',
    after: 'agent::review attaches as a new observer. github::pr::watch never changes.',
  },
  summary: {
    title: 'Built for C.O.D.E.R.',
    subtitle: 'Five properties that turn isolated software into a live execution system.',
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
  /* Reviewer: each tab links out to a use case page. Paths are placeholders until those pages exist. */
  tabs: [
    {
      id: 'harness',
      label: 'Agentic Harness',
      problem: "Agents shouldn't need bespoke tools.",
      solution:
        'Give an agent access to iii and the graph becomes its capability layer. One unified set of functionality from your harness through to your production environment.',
      fns: [
        'browser::navigate',
        'computer::click',
        'database::query',
        'sandbox::exec',
        'github::create_pr',
        'tailscale::devices',
        'search::web',
      ],
      href: '/use-cases/agentic-harness',
    },
    {
      id: 'platform',
      label: 'App Platform',
      problem: 'Backends grow boundaries faster than features.',
      solution:
        'Build backends from functions running across languages, processes, and machines. React to events and coordinate long-running execution.',
      fns: ['orders::create', 'pg::query', 'queue::push', 'cron::nightly', 'email::send', 'state::changed'],
      href: '/use-cases/app-platform',
    },
    {
      id: 'infra',
      label: 'Infrastructure',
      problem: 'Every new machine is a new integration.',
      solution:
        'Connect remote computation and infrastructure automatically. Compose models, retrieval, sandboxes, tools, and data infrastructure with zero effort.',
      fns: ['gpu::infer', 'vector::search', 'sandbox::run', 'tailscale::devices', 'k8s::scale', 's3::put'],
      href: '/use-cases/infrastructure',
    },
  ],
}

/**
 * Numbers. Live values (stars, contributors, workers) are fetched at render time; the rest are the doc's
 * placeholders until the team picks the figures. Keep the same set on Version B.
 */
export const numbers = {
  eyebrow: 'Numbers',
  title: 'Built to execute.',
  metrics: [
    { id: 'executions', value: 'XXM', label: 'function executions' },
    { id: 'overhead', value: 'XX µs', label: 'runtime overhead per call' },
    { id: 'discoverable', value: 'XXK', label: 'functions discoverable per engine' },
    { id: 'workers', value: null, label: 'workers in the registry' },
    { id: 'languages', value: '3', label: 'supported SDK languages' },
    { id: 'stars', value: null, label: 'GitHub stars' },
  ],
}

export const ownership = {
  eyebrow: 'Ownership',
  title: 'An architecture you own, not a vendor-locked platform.',
  subtitle: 'The convenience of a platform, without the lock-in.',
  copy: 'iii is an engine you run yourself: on your laptop, your cloud, or your hardware. Your code not only stays yours but can be trivially migrated anywhere. Any worker can be swapped out, forked, or replaced. When pricing changes or a vendor changes direction, your architecture still belongs to you.',
  environments: ['Local', 'Self-hosted', 'Cloud'],
}

export const finalCta = {
  title: 'Make anything callable.',
  copy: 'Start building your iii system.',
  primary: { label: 'Get started', href: 'https://iii.dev/docs/install' },
  secondary: { label: 'View GitHub', href: 'https://github.com/iii-hq/iii' },
}
