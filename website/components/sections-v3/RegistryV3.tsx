import { useState } from 'react';

interface RegistryV3Props {
  isDarkMode?: boolean;
}

type Worker = {
  id: string;
  name: string;
  description: string;
  category:
    | 'service'
    | 'agent'
    | 'browser'
    | 'sandbox'
    | 'integration'
    | 'data'
    | 'event'
    | 'your code';
  version: string;
  installs: string;
  official: boolean;
};

const WORKERS: Worker[] = [
  {
    id: 'iii/postgres',
    name: 'postgres',
    description: 'A service worker. Functions for query, transact; triggers on change streams.',
    category: 'service',
    version: '1.4.2',
    installs: '24.1k',
    official: true,
  },
  {
    id: 'iii/redis',
    name: 'redis',
    description: 'A service worker. Key/value, pub-sub, and streams as functions and triggers.',
    category: 'service',
    version: '1.2.0',
    installs: '18.7k',
    official: true,
  },
  {
    id: 'iii/agent',
    name: 'agent',
    description: 'An agent worker. LLMs with tools, memory, and streaming responses.',
    category: 'agent',
    version: '2.1.0',
    installs: '9.8k',
    official: true,
  },
  {
    id: 'iii/browser',
    name: 'browser',
    description: 'A browser worker. Drive a real browser; navigate, click, scrape as functions.',
    category: 'browser',
    version: '0.9.0',
    installs: '4.5k',
    official: true,
  },
  {
    id: 'iii/sandbox',
    name: 'sandbox',
    description: 'A sandbox worker. Run untrusted code in ephemeral microVMs.',
    category: 'sandbox',
    version: '0.7.3',
    installs: '3.1k',
    official: true,
  },
  {
    id: 'iii/queue',
    name: 'queue',
    description: 'An event worker. Durable, ordered queues with retries and DLQ.',
    category: 'event',
    version: '2.0.0',
    installs: '31.4k',
    official: true,
  },
  {
    id: 'iii/cron',
    name: 'cron',
    description: 'An event worker. Schedules functions on the right second, with locks.',
    category: 'event',
    version: '1.1.0',
    installs: '22.0k',
    official: true,
  },
  {
    id: 'iii/observability',
    name: 'observability',
    description: 'A service worker. OTLP traces, logs, and metrics for the whole graph.',
    category: 'data',
    version: '1.3.0',
    installs: '28.6k',
    official: true,
  },
  {
    id: 'community/openai',
    name: 'openai',
    description: 'An agent worker. Chat, embeddings, and tools — typed function wrappers.',
    category: 'agent',
    version: '1.5.0',
    installs: '11.7k',
    official: false,
  },
  {
    id: 'community/stripe',
    name: 'stripe',
    description: 'An integration worker. Charges, subscriptions, and webhook triggers.',
    category: 'integration',
    version: '1.0.0',
    installs: '5.9k',
    official: false,
  },
  {
    id: 'community/slack',
    name: 'slack',
    description: 'An integration worker. Post messages, react to events, slash commands as triggers.',
    category: 'integration',
    version: '1.1.0',
    installs: '4.0k',
    official: false,
  },
  {
    id: 'you/your-app',
    name: 'your-app',
    description: 'Your code, registered as a worker. Functions, triggers, traces — same as everything else.',
    category: 'your code',
    version: 'local',
    installs: '—',
    official: false,
  },
];

const CATEGORIES = [
  { key: 'all', label: 'All' },
  { key: 'service', label: 'Services' },
  { key: 'agent', label: 'Agents' },
  { key: 'browser', label: 'Browsers' },
  { key: 'sandbox', label: 'Sandboxes' },
  { key: 'event', label: 'Events' },
  { key: 'integration', label: 'Integrations' },
  { key: 'data', label: 'Data' },
  { key: 'your code', label: 'Your code' },
] as const;

type CategoryKey = (typeof CATEGORIES)[number]['key'];

export function RegistryV3({ isDarkMode = true }: RegistryV3Props) {
  const [active, setActive] = useState<CategoryKey>('all');
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const filtered =
    active === 'all' ? WORKERS : WORKERS.filter((w) => w.category === active);

  const copy = (worker: Worker) => {
    navigator.clipboard?.writeText(`iii worker add ${worker.name}`);
    setCopiedId(worker.id);
    setTimeout(() => setCopiedId(null), 1400);
  };

  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-black' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const cardHoverBorder = isDarkMode
    ? 'hover:border-iii-accent/60'
    : 'hover:border-iii-accent-light/60';
  const chipIdle = isDarkMode
    ? 'border-iii-light/15 text-iii-light/60 hover:text-iii-light hover:border-iii-light/30'
    : 'border-iii-black/15 text-iii-black/60 hover:text-iii-black hover:border-iii-black/30';
  const chipActive = isDarkMode
    ? 'border-iii-accent text-iii-accent'
    : 'border-iii-accent-light text-iii-accent-light';
  const codeBg = isDarkMode ? 'bg-iii-black/40' : 'bg-iii-light';

  return (
    <section id="registry" className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-14 md:py-24">
        <div className="max-w-2xl">
          <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
            The open set
          </p>
          <h2
            className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
          >
            Workers,{' '}
            <span className={accent}>by one command.</span>
          </h2>
          <p className={`mt-4 text-sm md:text-base ${secondary}`}>
            A worker is anything that does work and speaks to the engine — a
            binary, an image, a build, your local directory. Browse the
            registry, install one, install ten. Publish your own. The list
            never closes.
          </p>
        </div>

        <div className="mt-8 flex flex-wrap gap-2">
          {CATEGORIES.map((cat) => (
            <button
              key={cat.key}
              type="button"
              onClick={() => setActive(cat.key)}
              className={`px-3 py-1.5 rounded-full border text-[11px] tracking-wide transition-colors cursor-pointer ${
                active === cat.key ? chipActive : chipIdle
              }`}
            >
              {cat.label}
            </button>
          ))}
        </div>

        <div className="mt-6 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3 md:gap-4">
          {filtered.map((w) => (
            <article
              key={w.id}
              className={`rounded-xl border ${cardBorder} ${cardBg} ${cardHoverBorder} transition-colors p-5 flex flex-col gap-3`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h3
                      className={`text-base font-bold tracking-tight truncate ${primary}`}
                    >
                      {w.name}
                    </h3>
                    {w.official && (
                      <span
                        className={`text-[9px] uppercase tracking-[0.22em] px-1.5 py-0.5 rounded ${accent} border ${
                          isDarkMode
                            ? 'border-iii-accent/40'
                            : 'border-iii-accent-light/40'
                        }`}
                      >
                        official
                      </span>
                    )}
                  </div>
                  <p className={`mt-0.5 text-[10px] tracking-wide ${muted}`}>
                    {w.id} · v{w.version}
                  </p>
                </div>
                <span className={`text-[10px] tracking-wide shrink-0 ${muted}`}>
                  {w.installs === '—' ? '' : `${w.installs}/wk`}
                </span>
              </div>

              <p className={`text-xs leading-relaxed ${secondary}`}>
                {w.description}
              </p>

              <button
                type="button"
                onClick={() => copy(w)}
                className={`mt-auto flex items-center justify-between gap-2 px-3 py-2 rounded ${codeBg} border ${cardBorder} hover:border-current transition-colors text-left cursor-pointer group`}
              >
                <code className={`text-[11px] sm:text-xs ${primary}`}>
                  {copiedId === w.id ? (
                    <span className={`${accent}`}>copied!</span>
                  ) : (
                    <>
                      <span className={muted}>$ </span>
                      iii worker add {w.name}
                    </>
                  )}
                </code>
                <span
                  className={`text-[10px] uppercase tracking-[0.24em] ${muted} group-hover:${primary}`}
                >
                  copy
                </span>
              </button>
            </article>
          ))}
        </div>

        <div
          className={`mt-8 flex flex-col sm:flex-row sm:items-end sm:justify-between gap-3`}
        >
          <p className={`text-xs ${muted} max-w-xl`}>
            Categories are descriptive, not load-bearing. The model doesn’t
            care whether a worker is a database, an agent, or a thing that
            doesn’t have a category yet.
          </p>
          <a
            href="https://iii.dev/registry"
            className={`text-xs font-bold ${primary} hover:${accent} transition-colors inline-flex items-center gap-1`}
          >
            Browse the full registry <span>→</span>
          </a>
        </div>
      </div>
    </section>
  );
}
