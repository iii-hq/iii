import { LiveSystemAnimation } from './LiveSystemAnimation';

interface WhyIIISectionProps {
  isDarkMode?: boolean;
}

type Point = { title: string; body: string };

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
      title: 'Simple primitives',
      body: 'Collapse distributed backend design into a simple paradigm that humans and agents can reason about.',
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

const GROUP_LABELS = ['Execution model', 'Live system traits'];

function PatternsClosure({ isDarkMode }: { isDarkMode: boolean }) {
  const accent = isDarkMode ? 'text-iii-accent' : 'text-iii-accent-light';
  const primary = isDarkMode ? 'text-iii-light' : 'text-iii-black';
  const secondary = isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70';
  const muted = isDarkMode ? 'text-iii-light/45' : 'text-iii-black/45';
  const grid = isDarkMode ? 'rgba(244,244,244,0.05)' : 'rgba(0,0,0,0.05)';
  const border = isDarkMode ? 'border-iii-light/15' : 'border-iii-black/15';
  const panel = isDarkMode ? 'bg-iii-dark/25' : 'bg-white/55';

  return (
    <aside className="mx-auto max-w-7xl px-4 sm:px-6 pt-8 md:pt-10 pb-16 md:pb-20">
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
  const panelBorder = isDarkMode
    ? 'border-iii-light/15'
    : 'border-iii-black/15';
  const itemBorder = isDarkMode ? 'border-iii-light/10' : 'border-iii-black/10';
  const panelBg = isDarkMode ? 'bg-iii-dark/30' : 'bg-white/70';

  return (
    <section className={`w-full min-w-0 font-mono ${sectionBg}`}>
      <div className="mx-auto max-w-7xl px-4 sm:px-6 pt-8 md:pt-12 lg:pt-16 pb-6 md:pb-8">
        <div className="max-w-4xl mx-auto text-center">
          <h2
            className={`text-xl sm:text-3xl md:text-4xl lg:text-5xl font-bold tracking-tighter leading-[1.1] ${primary}`}
          >
            <span className={accent}>iii</span> in a nutshell
          </h2>
          <p
            className={`mt-3 text-sm md:text-base lg:text-lg leading-relaxed ${secondary}`}
          >
            iii's primitives result in an execution model and system traits that
            make it unreasonably good at creating backend software. In iii every
            capability, every framework, every tool can become a pattern on the
            same core system.
          </p>
        </div>
      </div>

      <div className="mx-auto max-w-7xl px-4 sm:px-6 pt-6 md:pt-8 pb-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 md:gap-8">
          {GROUPS.map((points, groupIdx) => {
            const startNumber = GROUPS.slice(0, groupIdx).reduce(
              (count, group) => count + group.length,
              1,
            );

            return (
              <section
                key={GROUP_LABELS[groupIdx]}
                className={`relative rounded-xl border ${panelBorder} ${panelBg} px-5 py-5 sm:px-6 sm:py-6`}
              >
                <div className="flex items-center justify-between">
                  <p
                    className={`font-mono text-[10px] uppercase tracking-[0.28em] ${muted}`}
                  >
                    {GROUP_LABELS[groupIdx]}
                  </p>
                  {GROUP_LABELS[groupIdx] === 'Live system traits' && (
                    <div className="absolute top-2 right-2 sm:top-4 sm:right-4 pointer-events-none opacity-80">
                      <LiveSystemAnimation />
                    </div>
                  )}
                </div>
                <div className="mt-4">
                  {points.map((point, pointIdx) => (
                    <article
                      key={point.title}
                      className={
                        pointIdx === 0 ? '' : `mt-5 pt-5 border-t ${itemBorder}`
                      }
                    >
                      <p
                        className={`font-mono text-[10px] uppercase tracking-[0.26em] ${muted}`}
                      >
                        {String(startNumber + pointIdx).padStart(2, '0')}
                      </p>
                      <h3
                        className={`mt-2 text-base sm:text-lg font-bold tracking-tight ${primary}`}
                      >
                        {point.title}
                      </h3>
                      <p
                        className={`mt-2 text-sm leading-relaxed ${secondary}`}
                      >
                        {point.body}
                      </p>
                    </article>
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      </div>
    </section>
  );
}
