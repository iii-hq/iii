interface AgentsV3Props {
  isDarkMode?: boolean;
}

const TRACE = [
  { name: 'http POST /tickets/handle', kind: 'trigger', total: '12ms', bar: 1, lang: '' },
  { name: 'tickets::handle', kind: 'TS · service', total: '1.31s', bar: 100, lang: 'ts' },
  { name: 'browser::screenshot', kind: 'browser tab', total: '742ms', bar: 56, lang: 'browser' },
  { name: 'ml::classify', kind: 'PY · model', total: '186ms', bar: 14, lang: 'py' },
  { name: 'agent::reply', kind: 'LLM agent', total: '294ms', bar: 22, lang: 'agent' },
  { name: 'crm::update', kind: 'RUST · worker', total: '38ms', bar: 3, lang: 'rs' },
];

export function AgentsV3({ isDarkMode = true }: AgentsV3Props) {
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const accentBg = isDarkMode ? 'bg-iii-accent' : 'bg-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-black' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const trackBg = isDarkMode ? 'bg-iii-light/10' : 'bg-iii-black/10';
  const rowBorder = isDarkMode ? 'border-iii-light/8' : 'border-iii-black/8';
  const tagBg = isDarkMode ? 'bg-iii-light/8' : 'bg-iii-black/8';

  return (
    <section className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-14 md:py-24">
        <div className="grid grid-cols-1 lg:grid-cols-[1.1fr_1.5fr] gap-10 items-start">
          <div>
            <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
              For AI / agent engineers
            </p>
            <h2
              className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
            >
              Agents, browsers, services —{' '}
              <span className={accent}>same model. one trace.</span>
            </h2>
            <p className={`mt-4 text-sm md:text-base ${secondary}`}>
              Agents aren’t a special feature. They’re workers like
              everything else. So is the browser tab they’re acting in. So is
              the Python model they call. So is the Rust service that writes
              the result to your CRM.
            </p>
            <ul className={`mt-5 space-y-2 text-sm ${secondary}`}>
              <li>
                <span className={accent}>·</span> The browser SDK runs in the
                tab itself — no shim, no headless gymnastics.
              </li>
              <li>
                <span className={accent}>·</span> Tools, memory, retries are
                triggers and functions, not framework concepts.
              </li>
              <li>
                <span className={accent}>·</span> One trace stitches every
                step end-to-end, regardless of language or location.
              </li>
            </ul>
          </div>

          <div
            className={`rounded-xl border ${cardBorder} ${cardBg} overflow-hidden`}
          >
            <div
              className={`flex items-center justify-between px-4 py-2.5 border-b ${cardBorder}`}
            >
              <span
                className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
              >
                Trace · trace_id 7c…b51
              </span>
              <span className={`text-[10px] uppercase tracking-[0.24em] ${accent}`}>
                live
              </span>
            </div>
            <div className="px-4 sm:px-5 py-4 sm:py-5">
              {TRACE.map((t, i) => (
                <div
                  key={t.name}
                  className={`grid grid-cols-[1.25rem_minmax(10rem,16rem)_1fr_auto] items-center gap-x-3 py-2 ${
                    i < TRACE.length - 1 ? `border-b ${rowBorder}` : ''
                  }`}
                >
                  <span className={muted}>
                    {i === 0 ? '┌─' : i === TRACE.length - 1 ? '└─' : '├─'}
                  </span>
                  <div className="min-w-0 flex items-center gap-2">
                    <code
                      className={`text-[12px] sm:text-[13px] truncate ${primary}`}
                    >
                      {t.name}
                    </code>
                    {t.kind && (
                      <span
                        className={`hidden sm:inline text-[9px] uppercase tracking-[0.22em] px-1.5 py-0.5 rounded ${tagBg} ${muted}`}
                      >
                        {t.kind}
                      </span>
                    )}
                  </div>
                  <div className={`relative h-1.5 rounded-full ${trackBg}`}>
                    <div
                      className={`absolute inset-y-0 left-0 rounded-full ${accentBg}`}
                      style={{ width: `${t.bar}%` }}
                    />
                  </div>
                  <span className={`${muted} tabular-nums shrink-0 text-[12px]`}>
                    {t.total}
                  </span>
                </div>
              ))}
            </div>
            <div
              className={`px-4 sm:px-5 py-3 border-t ${cardBorder} text-[11px] ${muted} flex items-center justify-between flex-wrap gap-2`}
            >
              <span>4 languages · 3 runtimes · 1 trace</span>
              <span>
                <span className={primary}>npx skillkit add iii-hq/iii/skills</span>{' '}
                · 27 skills for your coding agent
              </span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
