interface WhyIIISectionProps {
  isDarkMode?: boolean;
}

type Point = { title: string; body: string };

/** Six primitives in two rows: execution + surface, then live system traits. */
const GROUPS: Point[][] = [
  [
    {
      title: 'Durable orchestration',
      body: 'Coordinate long-running, failure-tolerant execution across workers, and triggers.',
    },
    {
      title: 'Interoperable execution',
      body: 'Execute across languages natively as if it were one runtime.',
    },
    {
      title: 'Minimal primitives',
      body: 'Collapse distributed backend design into a minimal primitive surface area that humans and agents can reason about.',
    },
  ],
  [
    {
      title: 'Live discovery',
      body: 'Functions and triggers exposed by one worker become visible and usable across the system in real time.',
    },
    {
      title: 'Live extensibility',
      body: 'Add new workers and capabilities to a live iii system without redesigning the architecture.',
    },
    {
      title: 'Live observability',
      body: 'Observe operations, traces, and system behavior across the entire connected stack in real time.',
    },
  ],
];

function StandinFigure({
  index,
  isDarkMode,
}: {
  index: number;
  isDarkMode: boolean;
}) {
  const stroke = isDarkMode ? 'rgba(243,247,36,0.35)' : 'rgba(47,127,255,0.4)';
  const grid = isDarkMode ? 'rgba(244,244,244,0.06)' : 'rgba(0,0,0,0.06)';
  const border = isDarkMode ? 'border-iii-light/15' : 'border-iii-black/15';
  const label = isDarkMode ? 'text-iii-light/35' : 'text-iii-black/40';

  return (
    <div
      className={`relative w-full aspect-[16/10] max-h-[min(280px,40vw)] mx-auto lg:mx-0 lg:max-h-none lg:aspect-[4/3] rounded-lg border ${border} bg-[length:24px_24px] overflow-hidden ${
        isDarkMode ? 'bg-iii-dark/40' : 'bg-white/50'
      }`}
      style={{
        backgroundImage: `linear-gradient(${grid} 1px, transparent 1px), linear-gradient(90deg, ${grid} 1px, transparent 1px)`,
      }}
      aria-hidden
    >
      <svg
        className="absolute inset-4 w-[calc(100%-2rem)] h-[calc(100%-2rem)]"
        viewBox="0 0 400 300"
        fill="none"
      >
        <path
          d="M20 20 L60 20 M20 20 L20 60 M380 20 L340 20 M380 20 L380 60 M20 280 L60 280 M20 280 L20 240 M380 280 L340 280 M380 280 L380 240"
          stroke={stroke}
          strokeWidth="2"
        />
        <rect
          x="120"
          y="100"
          width="160"
          height="100"
          rx="4"
          stroke={stroke}
          strokeWidth="1.5"
          strokeDasharray="6 4"
          fill="none"
        />
        <path
          d="M140 200 L200 130 L260 200"
          stroke={stroke}
          strokeWidth="1.2"
          opacity="0.6"
        />
        <circle
          cx="200"
          cy="115"
          r="8"
          stroke={stroke}
          strokeWidth="1.2"
          fill="none"
        />
      </svg>
      <span
        className={`absolute bottom-3 right-3 font-mono text-[10px] uppercase tracking-widest ${label}`}
      >
        Fig. {String(index + 1).padStart(2, '0')}
      </span>
    </div>
  );
}

/** Closing beat: same visual language as figures, centered — not a second intro block. */
function PatternsClosure({ isDarkMode }: { isDarkMode: boolean }) {
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/45' : 'text-iii-black/45';
  const grid = isDarkMode ? 'rgba(244,244,244,0.05)' : 'rgba(0,0,0,0.05)';
  const border = isDarkMode ? 'border-iii-light/15' : 'border-iii-black/15';
  const panel = isDarkMode ? 'bg-iii-dark/25' : 'bg-white/55';

  return (
    <aside className="mx-auto max-w-7xl px-4 sm:px-6 pt-4 md:pt-6 pb-16 md:pb-20">
      <div
        className={`relative mx-auto max-w-xl rounded-lg border ${border} ${panel} px-8 py-10 sm:px-10 sm:py-11 text-center bg-[length:20px_20px]`}
        style={{
          backgroundImage: `linear-gradient(${grid} 1px, transparent 1px), linear-gradient(90deg, ${grid} 1px, transparent 1px)`,
        }}
      >
        <div
          className={`mx-auto mb-6 h-px w-10 ${
            isDarkMode ? 'bg-iii-accent/55' : 'bg-iii-accent-light/55'
          }`}
          aria-hidden
        />
        <p
          className={`font-mono text-[10px] uppercase tracking-[0.35em] ${muted} mb-4`}
        >
          Composition
        </p>
        <p
          className={`font-chivo text-lg sm:text-xl md:text-2xl font-bold tracking-tight leading-snug ${primary}`}
        >
          <span className={accent}>Capabilities</span> become patterns
        </p>
        <p className={`mt-5 text-xs sm:text-sm leading-relaxed ${secondary}`}>
          Agents, frameworks, tools, and higher-level abstractions can all be
          built on top of the same core system.
        </p>
      </div>
    </aside>
  );
}

export function WhyIIISection({ isDarkMode = true }: WhyIIISectionProps) {
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/75' : 'text-iii-black/75';
  const muted = isDarkMode ? 'text-iii-light/50' : 'text-iii-black/50';
  const rule = isDarkMode
    ? 'border-iii-light/[0.08]'
    : 'border-iii-black/[0.08]';
  const sectionBg = isDarkMode ? 'bg-iii-black' : 'bg-iii-light';

  return (
    <section
      className={`w-full min-w-0 border-t font-mono ${rule} ${sectionBg}`}
    >
      <div className="mx-auto max-w-7xl px-4 sm:px-6 pt-16 md:pt-24 lg:pt-28 pb-4 md:pb-6">
        <div className="max-w-2xl">
          <h2
            className={`font-chivo text-3xl sm:text-4xl md:text-5xl font-bold tracking-tighter ${primary}`}
          >
            Why <span className={accent}>iii</span>?
          </h2>
          <p
            className={`mt-6 text-sm sm:text-base leading-relaxed ${secondary}`}
          >
            iii turns distributed backend complexity into a minimal set of
            real-time, interoperable primitives. Workers, functions, and
            triggers connect, discover and observe each other, and coordinate
            execution across languages as if it was one runtime.
          </p>
        </div>
      </div>

      <div className="mx-auto max-w-7xl px-4 sm:px-6">
        {GROUPS.map((points, groupIdx) => {
          const indexBase = GROUPS.slice(0, groupIdx).reduce(
            (acc, g) => acc + g.length,
            0,
          );
          return (
            <article
              key={groupIdx}
              className={`grid grid-cols-1 lg:grid-cols-2 gap-8 lg:gap-12 items-start py-10 md:py-14 border-t ${rule}`}
            >
              <div className="min-w-0">
                {points.map((item, j) => {
                  const n = indexBase + j + 1;
                  return (
                    <div
                      key={item.title}
                      className={
                        j > 0
                          ? `pt-8 border-t ${isDarkMode ? 'border-iii-light/10' : 'border-iii-black/10'}`
                          : ''
                      }
                    >
                      <p
                        className={`font-mono text-[10px] uppercase tracking-[0.3em] mb-2 ${muted}`}
                      >
                        {String(n).padStart(2, '0')}
                      </p>
                      <h3
                        className={`text-base sm:text-lg font-bold tracking-tight ${primary}`}
                      >
                        {item.title}
                      </h3>
                      <p
                        className={`mt-2 text-sm leading-relaxed ${secondary} max-w-xl`}
                      >
                        {item.body}
                      </p>
                    </div>
                  );
                })}
              </div>
              <div className="min-w-0 lg:pt-1">
                <StandinFigure index={groupIdx} isDarkMode={isDarkMode} />
              </div>
            </article>
          );
        })}
      </div>

      <PatternsClosure isDarkMode={isDarkMode} />
    </section>
  );
}
