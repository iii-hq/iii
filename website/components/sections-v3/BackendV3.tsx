interface BackendV3Props {
  isDarkMode?: boolean;
}

const TODAY = [
  'API gateway',
  'HTTP framework',
  'Background queue',
  'Scheduler',
  'Pub/sub',
  'KV / state store',
  'Stream / WebSocket layer',
  'Workflow engine',
  'Auth middleware',
  'Trace pipeline',
  'Vendor dashboards',
  'Glue code between all of them',
];

const TOMORROW = [
  'http worker',
  'queue worker',
  'cron worker',
  'pubsub worker',
  'state worker',
  'stream worker',
  'observability worker',
  'your own service worker',
  '...and the next thing',
];

export function BackendV3({ isDarkMode = true }: BackendV3Props) {
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-dark/30' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const dimText = isDarkMode ? 'text-iii-light/55' : 'text-iii-black/55';

  return (
    <section className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-14 md:py-24">
        <div className="grid grid-cols-1 lg:grid-cols-[1fr_1.4fr] gap-10 items-start">
          <div>
            <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
              For backend engineers
            </p>
            <h2
              className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
            >
              Your stack is already a bunch of workers —{' '}
              <span className={accent}>they just don’t know it.</span>
            </h2>
            <p className={`mt-4 text-sm md:text-base ${secondary}`}>
              Every layer you stitch together is a separate runtime with its
              own contract, its own failure mode, its own dashboard. iii puts
              all of them — and your services, and the next layer that
              doesn’t exist yet — under one model: register a function, bind a
              trigger, get a trace.
            </p>
            <p className={`mt-3 text-sm md:text-base ${secondary}`}>
              Nothing in the registry is special. Your code is a worker. The
              queue is a worker. The thing you’ll need next quarter is a
              worker. The model doesn’t care which.
            </p>
          </div>

          <div
            className={`rounded-xl border ${cardBorder} ${cardBg} overflow-hidden`}
          >
            <div className="grid grid-cols-1 md:grid-cols-2">
              <div
                className={`p-5 md:p-6 border-b md:border-b-0 md:border-r ${cardBorder}`}
              >
                <p
                  className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
                >
                  Today
                </p>
                <p className={`mt-2 text-sm ${dimText}`}>N runtimes, N contracts</p>
                <ul className="mt-4 space-y-1.5">
                  {TODAY.map((t) => (
                    <li
                      key={t}
                      className={`text-[12px] sm:text-[13px] ${dimText} flex items-start gap-2`}
                    >
                      <span className={muted}>·</span>
                      <span className="line-through decoration-iii-light/30">
                        {t}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
              <div className="p-5 md:p-6">
                <p
                  className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
                >
                  With iii
                </p>
                <p className={`mt-2 text-sm ${secondary}`}>
                  One model. Workers all the way down.
                </p>
                <ul className="mt-4 space-y-1.5">
                  {TOMORROW.map((t) => (
                    <li
                      key={t}
                      className={`text-[12px] sm:text-[13px] ${primary} flex items-start gap-2`}
                    >
                      <span className={accent}>·</span>
                      <span>{t}</span>
                    </li>
                  ))}
                </ul>
                <p className={`mt-5 text-[11px] ${muted}`}>
                  Same registration. Same trigger. Same trace.
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
