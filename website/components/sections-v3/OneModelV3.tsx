interface OneModelV3Props {
  isDarkMode?: boolean;
}

type Demo = {
  primitive: 'Function' | 'Trigger' | 'Worker';
  headline: string;
  body: string;
  rows: { label: string; value: string }[];
  caption: string;
};

const DEMOS: Demo[] = [
  {
    primitive: 'Function',
    headline: 'Anything is a Function.',
    body: 'A typed input. An optional output. A globally unique ID. Implemented by code, by a worker, or wrapped around something that already exists.',
    rows: [
      { label: 'orders::handle', value: 'local TypeScript handler' },
      { label: 'ml::classify', value: 'remote Python worker' },
      { label: 'stripe::charge', value: 'third-party HTTP API, wrapped' },
      { label: 'sandbox::run', value: 'a worker that does not exist yet' },
    ],
    caption:
      'Same call site every time:  await iii.trigger(id, input)',
  },
  {
    primitive: 'Trigger',
    headline: 'Anything is a Trigger.',
    body: 'HTTP, cron, queue, stream, state change, or an event source contributed by a worker. The same Function fires from completely different things, no code change.',
    rows: [
      { label: 'http', value: 'POST /orders → orders::handle' },
      { label: 'cron', value: '0 9 * * *  → reports::daily' },
      { label: 'queue', value: 'durable subscriber → orders::handle' },
      { label: 'state', value: 'on change of users::*  → audit::log' },
    ],
    caption: 'New trigger types are contributed by workers, not patched in.',
  },
  {
    primitive: 'Worker',
    headline: 'Anything is a Worker.',
    body: 'A service, an agent, a browser tab, a sandbox, a CLI, a device. Same registration line in TypeScript, Python, Rust, or the browser SDK.',
    rows: [
      { label: 'service', value: 'your Node / Python / Rust process' },
      { label: 'agent', value: 'an LLM with tools and memory' },
      { label: 'browser', value: 'the iii SDK running in a tab' },
      { label: 'sandbox', value: 'untrusted code in a microVM' },
    ],
    caption:
      'The set is open. The model never changes when something new shows up.',
  },
];

export function OneModelV3({ isDarkMode = true }: OneModelV3Props) {
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-black' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const rowBorder = isDarkMode ? 'border-iii-light/8' : 'border-iii-black/8';

  return (
    <section id="model" className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-14 md:py-24">
        <div className="max-w-3xl">
          <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
            One model
          </p>
          <h2
            className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
          >
            Three nouns close the model.
            <br />
            <span className={accent}>The set never closes.</span>
          </h2>
          <p className={`mt-4 text-sm md:text-base ${secondary}`}>
            Function, Trigger, Worker. Every capability your backend needs
            today, and every one it doesn’t have yet, fits one of three
            shapes. The vocabulary stays the same as the system grows.
          </p>
        </div>

        <div className="mt-10 grid grid-cols-1 lg:grid-cols-3 gap-4 md:gap-5">
          {DEMOS.map((d) => (
            <article
              key={d.primitive}
              className={`rounded-xl border ${cardBorder} ${cardBg} p-5 md:p-6 flex flex-col gap-4`}
            >
              <div className="flex items-baseline justify-between">
                <span
                  className={`text-[10px] uppercase tracking-[0.28em] ${muted}`}
                >
                  primitive
                </span>
                <span
                  className={`text-xs uppercase tracking-[0.22em] ${accent}`}
                >
                  {d.primitive}
                </span>
              </div>
              <h3
                className={`text-xl md:text-2xl font-bold tracking-tight font-chivo ${primary}`}
              >
                {d.headline}
              </h3>
              <p className={`text-sm leading-relaxed ${secondary}`}>{d.body}</p>

              <div className={`mt-2 border-t ${rowBorder}`}>
                {d.rows.map((row) => (
                  <div
                    key={row.label}
                    className={`grid grid-cols-[minmax(7rem,9rem)_1fr] gap-x-3 py-2 border-b ${rowBorder} text-[12px] sm:text-[13px]`}
                  >
                    <code className={`${primary} truncate`}>{row.label}</code>
                    <span className={`${secondary} truncate`}>{row.value}</span>
                  </div>
                ))}
              </div>

              <p className={`mt-1 text-[11px] ${muted}`}>{d.caption}</p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}
