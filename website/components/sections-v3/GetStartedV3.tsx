import { InstallShButton } from '../InstallShButton';

interface GetStartedV3Props {
  isDarkMode?: boolean;
}

const SDK_LINKS = [
  { lang: 'TypeScript', cmd: 'npm install iii-sdk', href: 'https://www.npmjs.com/package/iii-sdk' },
  { lang: 'Python', cmd: 'pip install iii-sdk', href: 'https://pypi.org/project/iii-sdk/' },
  { lang: 'Rust', cmd: 'cargo add iii-sdk', href: 'https://crates.io/crates/iii-sdk' },
  { lang: 'Browser', cmd: 'npm install iii-browser-sdk', href: 'https://www.npmjs.com/package/iii-browser-sdk' },
];

export function GetStartedV3({ isDarkMode = true }: GetStartedV3Props) {
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-black' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const codeBg = isDarkMode ? 'bg-iii-black/40' : 'bg-iii-light';

  return (
    <section className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-16 md:py-24">
        <div className="max-w-3xl">
          <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
            Get started
          </p>
          <h2
            className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
          >
            Run an engine. Add a worker.{' '}
            <span className={accent}>Compose anything.</span>
          </h2>
        </div>

        <div className="mt-10 grid grid-cols-1 md:grid-cols-3 gap-4 md:gap-5">
          <article
            className={`rounded-xl border ${cardBorder} ${cardBg} p-5 md:p-6 flex flex-col gap-4`}
          >
            <span
              className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
            >
              01 · Run the engine
            </span>
            <p className={`text-sm ${secondary}`}>
              One binary. One config. Hot reload built in.
            </p>
            <div className="mt-auto">
              <InstallShButton isDarkMode={isDarkMode} />
            </div>
          </article>

          <article
            className={`rounded-xl border ${cardBorder} ${cardBg} p-5 md:p-6 flex flex-col gap-4`}
          >
            <span
              className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
            >
              02 · Add a worker
            </span>
            <p className={`text-sm ${secondary}`}>
              From the registry, an OCI image, a binary, or a local directory.
            </p>
            <div
              className={`mt-auto px-3 py-2.5 rounded ${codeBg} border ${cardBorder}`}
            >
              <code className={`text-[12px] sm:text-sm ${primary}`}>
                <span className={muted}>$ </span>iii worker add{' '}
                <span className={accent}>postgres</span>
              </code>
            </div>
          </article>

          <article
            className={`rounded-xl border ${cardBorder} ${cardBg} p-5 md:p-6 flex flex-col gap-4`}
          >
            <span
              className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
            >
              03 · Or write your own
            </span>
            <p className={`text-sm ${secondary}`}>
              Your code is a worker too. Pick a language and start registering.
            </p>
            <ul className="mt-auto space-y-1.5">
              {SDK_LINKS.map((sdk) => (
                <li key={sdk.lang}>
                  <a
                    href={sdk.href}
                    target="_blank"
                    rel="noreferrer noopener"
                    className={`flex items-center justify-between gap-2 px-3 py-2 rounded ${codeBg} border ${cardBorder} hover:border-current transition-colors group`}
                  >
                    <code className={`text-[11px] sm:text-xs ${primary}`}>
                      <span className={muted}>$ </span>
                      {sdk.cmd}
                    </code>
                    <span
                      className={`text-[10px] uppercase tracking-[0.22em] ${muted} group-hover:${accent}`}
                    >
                      {sdk.lang} →
                    </span>
                  </a>
                </li>
              ))}
            </ul>
          </article>
        </div>

        <div
          className={`mt-12 pt-6 border-t ${cardBorder} flex flex-col md:flex-row md:items-center md:justify-between gap-3 text-[11px]`}
        >
          <span className={`uppercase tracking-[0.32em] ${muted}`}>
            One model · An open set of workers · Same trace across all of them
          </span>
          <span className={muted}>
            <span className={primary}>engine</span> ELv2 ·{' '}
            <span className={primary}>SDKs</span> Apache‑2.0
          </span>
        </div>
      </div>
    </section>
  );
}
