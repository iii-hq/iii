interface PlatformV3Props {
  isDarkMode?: boolean;
}

type Capability = {
  tag: string;
  title: string;
  body: string;
  proof: string;
};

const CAPS: Capability[] = [
  {
    tag: 'Isolation',
    title: 'Workers in real microVMs.',
    body: '`iii worker add` resolves an OCI image, a binary, or a local directory and runs it inside a libkrun microVM with its own rootfs.',
    proof: 'iii worker add ./untrusted-thing  →  vm.pid  →  registered',
  },
  {
    tag: 'Federation',
    title: 'One model, many engines.',
    body: 'A worker can be a remote iii engine. Functions and triggers in one cluster show up in another with the same identity and the same trace.',
    proof: 'iii-bridge → engine.prod  ✓  219 functions exposed',
  },
  {
    tag: 'Hot reload',
    title: 'Edit. Save. It’s live.',
    body: 'The HTTP route table swaps in place. The worker set reloads from yaml. Local-dir workers re-sync into the VM. No restart, no dropped connections.',
    proof: 'config.yaml changed  →  3 routes updated  ·  1 worker reloaded',
  },
];

export function PlatformV3({ isDarkMode = true }: PlatformV3Props) {
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/40' : 'text-iii-black/40';
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const sectionBg = isDarkMode ? 'bg-iii-dark/30' : 'bg-iii-light';
  const cardBorder = isDarkMode
    ? 'border-iii-light/12'
    : 'border-iii-black/12';
  const cardBg = isDarkMode ? 'bg-iii-dark/50' : 'bg-white/80';
  const codeBg = isDarkMode ? 'bg-iii-black/40' : 'bg-iii-light';

  return (
    <section className={`w-full font-mono ${sectionBg}`}>
      <div className="mx-auto w-full max-w-6xl px-4 sm:px-6 py-14 md:py-24">
        <div className="max-w-3xl">
          <p className={`text-[10px] uppercase tracking-[0.32em] ${muted}`}>
            For platform engineers
          </p>
          <h2
            className={`mt-3 text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter leading-[1.05] font-chivo ${primary}`}
          >
            One model. <span className={accent}>Many implementations.</span>
          </h2>
          <p className={`mt-4 text-sm md:text-base ${secondary}`}>
            Workers can be safely isolated, federated across engines, and hot
            reloaded in place. Because the model is uniform, the way you ship
            them can vary without changing how they’re called.
          </p>
        </div>

        <div className="mt-10 grid grid-cols-1 md:grid-cols-3 gap-4 md:gap-5">
          {CAPS.map((c) => (
            <article
              key={c.tag}
              className={`rounded-xl border ${cardBorder} ${cardBg} p-5 md:p-6 flex flex-col gap-4`}
            >
              <span
                className={`text-[10px] uppercase tracking-[0.28em] ${accent}`}
              >
                {c.tag}
              </span>
              <h3
                className={`text-xl md:text-2xl font-bold tracking-tight font-chivo ${primary}`}
              >
                {c.title}
              </h3>
              <p className={`text-sm leading-relaxed ${secondary}`}>{c.body}</p>
              <div
                className={`mt-auto px-3 py-2 rounded ${codeBg} border ${cardBorder}`}
              >
                <code className={`text-[11px] sm:text-xs ${primary}`}>
                  <span className={muted}>$ </span>
                  {c.proof}
                </code>
              </div>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}
