import { useEffect, useState } from 'react';
import { InstallShButton } from '../InstallShButton';

interface HeroV3Props {
  isDarkMode?: boolean;
}

type WorkerKind = {
  label: string;
  command: string;
  detail: string;
  hint: string;
};

const KINDS: WorkerKind[] = [
  {
    label: 'a service',
    command: 'iii worker add postgres',
    detail: 'postgres@1.4.2 — Functions: query, transact · Triggers: change',
    hint: 'a binary',
  },
  {
    label: 'an agent',
    command: 'iii worker add agent',
    detail: 'agent@2.1.0 — Functions: reply, plan · Triggers: tool-call',
    hint: 'a build',
  },
  {
    label: 'a browser',
    command: 'iii worker add browser',
    detail: 'browser@0.9.0 — Functions: navigate, screenshot, scrape',
    hint: 'a runtime',
  },
  {
    label: 'a sandbox',
    command: 'iii worker add sandbox',
    detail: 'sandbox@0.7.3 — Functions: run · isolation: libkrun',
    hint: 'a microVM',
  },
  {
    label: 'your own service',
    command: 'iii worker add ./my-service',
    detail: 'my-service@local — Functions: orders::handle, orders::refund',
    hint: 'your code',
  },
  {
    label: 'anything you build next',
    command: 'iii worker add ./next-thing',
    detail: 'the model never changes — registration, trigger, trace, compose',
    hint: 'open set',
  },
];

export function HeroV3({ isDarkMode = true }: HeroV3Props) {
  const [idx, setIdx] = useState(0);

  useEffect(() => {
    const t = setInterval(() => setIdx((i) => (i + 1) % KINDS.length), 2400);
    return () => clearInterval(t);
  }, []);

  const kind = KINDS[idx];

  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const accentBg = isDarkMode ? 'bg-iii-accent' : 'bg-iii-accent-light';
  const success = 'text-iii-success';
  const promptColor = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const termBg = isDarkMode ? 'bg-iii-dark/80' : 'bg-white';
  const termBorder = isDarkMode
    ? 'border-iii-light/15'
    : 'border-iii-black/15';
  const termHeader = isDarkMode ? 'bg-iii-black/60' : 'bg-iii-light';

  return (
    <section className="relative w-full font-mono">
      <div className="relative z-10 w-full max-w-6xl mx-auto px-4 sm:px-6 py-10 md:py-16">
        <div className="flex flex-col items-center text-center gap-7 md:gap-10">
          <div
            className={`inline-flex items-center gap-2 px-3 py-1.5 rounded-full border text-[10px] sm:text-xs uppercase tracking-[0.28em] ${termBorder} ${muted}`}
          >
            <span className={`inline-block w-1.5 h-1.5 rounded-full ${accentBg}`} />
            <span>function · trigger · worker</span>
          </div>

          <h1
            className={`font-chivo font-bold tracking-tighter leading-[0.95] text-[clamp(2rem,6vw,4.75rem)] ${primary}`}
          >
            Anything that does work
            <br />
            is a <span className={accent}>worker</span>.
          </h1>

          <p
            className={`max-w-[68ch] text-base sm:text-lg leading-relaxed ${secondary}`}
          >
            iii is a small engine and an open registry of workers. Functions,
            Triggers, and Workers are the entire vocabulary — and any
            capability you can name fits in it. Same registration. Same
            trigger. Same trace. Same composition. Across every language,
            location, and runtime.
          </p>

          <div
            className={`w-full max-w-3xl rounded-xl border ${termBorder} ${termBg} overflow-hidden text-left shadow-2xl shadow-black/20`}
          >
            <div
              className={`flex items-center gap-2 px-4 py-2.5 border-b ${termBorder} ${termHeader}`}
            >
              <div className="flex gap-1.5">
                <span className="w-2.5 h-2.5 rounded-full bg-red-500/80" />
                <span className="w-2.5 h-2.5 rounded-full bg-yellow-500/80" />
                <span className="w-2.5 h-2.5 rounded-full bg-green-500/80" />
              </div>
              <span
                className={`ml-3 text-[10px] uppercase tracking-[0.28em] ${muted}`}
              >
                ~ /your-app
              </span>
              <span className="ml-auto text-[10px] uppercase tracking-[0.24em] flex items-center gap-2">
                <span className={muted}>add</span>
                <span className={`${primary}`}>{kind.label}</span>
                <span className={`${muted}`}>·</span>
                <span className={accent}>{kind.hint}</span>
              </span>
            </div>

            <div className="px-4 sm:px-5 py-5 text-[12px] sm:text-sm leading-relaxed min-h-[7.5rem]">
              <div className="grid grid-cols-[auto_1fr] gap-x-2">
                <span className={promptColor}>$</span>
                <span className={primary}>
                  iii worker add{' '}
                  <span className={accent}>
                    {kind.command.replace('iii worker add ', '')}
                  </span>
                </span>
              </div>
              <div
                className="grid grid-cols-[auto_1fr] gap-x-2"
                style={{ marginTop: '0.5rem' }}
              >
                <span className={success}>✓</span>
                <span className={secondary}>{kind.detail}</span>
              </div>
              <div
                className="grid grid-cols-[auto_1fr] gap-x-2"
                style={{ marginTop: '0.85rem' }}
              >
                <span className={promptColor}>$</span>
                <span className={primary}>
                  <span
                    className={`inline-block w-[0.5em] -mb-[0.1em] ${accentBg} animate-pulse`}
                    style={{ height: '1.05em', verticalAlign: 'middle' }}
                  />
                </span>
              </div>
            </div>
          </div>

          <div className="flex flex-col sm:flex-row gap-3 sm:gap-4 items-center justify-center w-full max-w-xl">
            <InstallShButton isDarkMode={isDarkMode} />
            <a
              href="#model"
              className={`group inline-flex items-center justify-center gap-2 px-4 py-2.5 md:py-3 rounded border text-xs md:text-sm font-bold transition-colors w-full sm:w-auto ${
                isDarkMode
                  ? 'border-iii-light/30 text-iii-light hover:border-iii-accent hover:text-iii-accent'
                  : 'border-iii-black/30 text-iii-black hover:border-iii-accent-light hover:text-iii-accent-light'
              }`}
            >
              See the model
              <span className="transition-transform group-hover:translate-x-0.5">
                ↓
              </span>
            </a>
          </div>
        </div>
      </div>
    </section>
  );
}
