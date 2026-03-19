import { DemoSequencer, homepageFlow } from '../demo';

interface InteractiveDemoFlowSectionProps {
  isDarkMode?: boolean;
}

export function InteractiveDemoFlowSection({
  isDarkMode = true,
}: InteractiveDemoFlowSectionProps) {
  return (
    <section className="w-full">
      <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl mx-auto py-8 md:py-12 lg:py-20">
        <div className="max-w-4xl mx-auto text-center mb-6 md:mb-8">
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

        <DemoSequencer steps={homepageFlow} mode="hero" />
      </div>
    </section>
  );
}
