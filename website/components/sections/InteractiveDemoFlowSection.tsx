import { DemoSequencer, homepagePlaceholderFlow } from "../demo";

interface InteractiveDemoFlowSectionProps {
  isDarkMode?: boolean;
}

export function InteractiveDemoFlowSection({
  isDarkMode = true,
}: InteractiveDemoFlowSectionProps) {
  return (
    <section className="w-full">
      <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl mx-auto py-8 md:py-12 lg:py-20">
        <div className="max-w-4xl mb-6 md:mb-8">
          <p
            className={`text-xs uppercase tracking-wider mb-2 ${
              isDarkMode ? "text-iii-accent" : "text-iii-accent-light"
            }`}
          >
            Interactive Placeholder Flow
          </p>
          <h2
            className={`text-xl sm:text-2xl md:text-3xl font-bold font-chivo ${
              isDarkMode ? "text-iii-light" : "text-iii-black"
            }`}
          >
            Click through the orchestration story
          </h2>
          <p
            className={`mt-2 text-sm md:text-base ${
              isDarkMode ? "text-iii-light/70" : "text-iii-black/70"
            }`}
          >
            This is intentionally placeholder content: Slack-like request, optional
            reply composer, line-by-line Write it editor, and status checkpoints.
            Each step drives the next.
          </p>
        </div>

        <DemoSequencer steps={homepagePlaceholderFlow} mode="hero" />
      </div>
    </section>
  );
}
