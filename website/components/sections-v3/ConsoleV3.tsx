interface ConsoleV3Props {
  isDarkMode?: boolean;
}

const ACTIONS = [
  { verb: 'invoke', what: 'any function from the UI', kind: 'function' },
  { verb: 'publish', what: 'a JSON message to a queue', kind: 'queue' },
  { verb: 'redrive', what: 'a single message or the whole DLQ', kind: 'queue' },
  { verb: 'retry', what: 'a failed step from inside a trace', kind: 'trace' },
  { verb: 'inspect', what: 'live workers, traces, streams, state', kind: 'system' },
];

const PANELS = [
  { name: 'workers', detail: '14 connected · 2 isolating · 1 reloading' },
  { name: 'functions', detail: '219 registered · 7 namespaces' },
  { name: 'triggers', detail: 'http · cron · queue · stream · state · custom' },
  { name: 'traces', detail: 'live · retry from any span' },
  { name: 'queues', detail: '4 topics · DLQ redrive · publish' },
  { name: 'state', detail: 'scopes · reactive triggers' },
];

export function ConsoleV3({ isDarkMode = true }: ConsoleV3Props) {
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const accentBg = isDarkMode ? 'bg-iii-accent' : 'bg-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-dark/30' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const rowBorder = isDarkMode ? 'border-iii-light/8' : 'border-iii-black/8';
  const dotMuted = isDarkMode ? 'bg-iii-light/30' : 'bg-iii-black/30';

  return (
    <section className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-14 md:py-24">
        <div className="grid grid-cols-1 lg:grid-cols-[1fr_1.3fr] gap-10 items-start">
          <div>
            <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
              Console
            </p>
            <h2
              className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
            >
              Operate the system,{' '}
              <span className={accent}>not just observe it.</span>
            </h2>
            <p className={`mt-4 text-sm md:text-base ${secondary}`}>
              The iii console isn’t a dashboard. It’s an operations surface.
              Because every worker speaks the same model, every action works
              on every worker — your code or anything else.
            </p>
            <ul className="mt-6 space-y-2">
              {ACTIONS.map((a) => (
                <li
                  key={a.verb}
                  className="grid grid-cols-[5.5rem_1fr] gap-x-3 items-center"
                >
                  <span
                    className={`text-[11px] uppercase tracking-[0.22em] ${accent}`}
                  >
                    {a.verb}
                  </span>
                  <span className={`text-sm ${secondary}`}>{a.what}</span>
                </li>
              ))}
            </ul>
          </div>

          <div
            className={`rounded-xl border ${cardBorder} ${cardBg} overflow-hidden`}
          >
            <div
              className={`flex items-center justify-between px-4 py-2.5 border-b ${cardBorder}`}
            >
              <div className="flex items-center gap-2">
                <span className={`inline-block w-1.5 h-1.5 rounded-full ${accentBg} animate-pulse`} />
                <span
                  className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
                >
                  iii console · localhost:5173
                </span>
              </div>
              <span className={`text-[10px] uppercase tracking-[0.24em] ${accent}`}>
                live
              </span>
            </div>
            <ul className="divide-y divide-transparent">
              {PANELS.map((p, i) => (
                <li
                  key={p.name}
                  className={`grid grid-cols-[1.25rem_minmax(7rem,9rem)_1fr_auto] items-center gap-x-3 px-4 py-3 ${
                    i < PANELS.length - 1 ? `border-b ${rowBorder}` : ''
                  }`}
                >
                  <span className={`inline-block w-1.5 h-1.5 rounded-full ${dotMuted}`} />
                  <code className={`${primary} text-[12px] sm:text-[13px]`}>
                    /{p.name}
                  </code>
                  <span className={`text-[12px] ${secondary} truncate`}>
                    {p.detail}
                  </span>
                  <span
                    className={`text-[10px] uppercase tracking-[0.22em] ${muted}`}
                  >
                    open
                  </span>
                </li>
              ))}
            </ul>
            <div
              className={`px-4 py-3 border-t ${cardBorder} text-[11px] ${muted} flex items-center justify-between flex-wrap gap-2`}
            >
              <span>One UI for every worker in the registry.</span>
              <span>
                <span className={primary}>iii console</span>
              </span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
