import { DemoSequencer, homepageFlow } from '../demo';

interface InteractiveDemoFlowSectionProps {
  isDarkMode?: boolean;
}

export function InteractiveDemoFlowSection({
  isDarkMode = true,
}: InteractiveDemoFlowSectionProps) {
  return (
    <section className="w-full h-full">
      <div className="w-full h-full max-w-7xl mx-auto px-4 sm:px-6 py-6 md:py-7 grid grid-rows-[auto_minmax(0,1fr)] gap-4 md:gap-6">
        <div className="max-w-4xl mx-auto text-center">
          <h2
            className={`text-xl sm:text-3xl md:text-4xl lg:text-5xl font-bold tracking-tighter leading-[1.1] ${
              isDarkMode ? 'text-iii-light' : 'text-iii-black'
            }`}
          >
            The iii experience
          </h2>
          <p
            className={`mt-3 text-sm md:text-base lg:text-lg leading-relaxed ${
              isDarkMode ? 'text-iii-light/70' : 'text-iii-black/70'
            }`}
          >
            iii makes it unreasonably simple to write and extend backend
            software.
          </p>
        </div>

        <DemoSequencer
          steps={homepageFlow}
          mode="hero"
          className="!h-full !min-h-0 w-full max-h-full"
        />
      </div>
    </section>
  );
}
