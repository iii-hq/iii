const catchCards = [
  {
    icon: "📚",
    title: "Learning curve",
    description:
      "iii introduces new concepts and ways of thinking about infrastructure. We have extensive documentation and guides to help you get started.",
    details: [
      "Most developers get productive in 1-2 weeks",
      "Excellent docs and interactive examples",
      "Active community for support",
      "Familiar concepts, unified approach",
    ],
  },
  {
    icon: "✨",
    title: "Different approach",
    description:
      "iii unifies your stack in a new way. If you're used to managing multiple tools separately, this unified approach takes adjustment.",
    details: [
      "Single pane of glass for everything",
      "Unified orchestration model",
      "Works alongside existing infrastructure",
      "Migrate at your own pace",
    ],
  },
  {
    icon: "⚙️",
    title: "New mental model",
    description:
      "You need to think differently about infrastructure and orchestration. This is actually a feature, not a bug.",
    details: [
      "Infrastructure as composition",
      "Explicit dependencies",
      "Unified control plane",
      "Better observability and debugging",
    ],
  },
];

function CatchCard({
  icon,
  title,
  description,
  details,
}: {
  key?: string | number;
  icon: string;
  title: string;
  description: string;
  details: string[];
}) {
  return (
    <div className="group p-8 rounded-lg border border-neutral-800 bg-neutral-900/30 hover:border-neutral-700 transition-all hover:bg-neutral-900/50">
      {/* Header */}
      <div className="space-y-3 mb-6">
        <div className="flex items-center gap-3">
          <span className="text-3xl">{icon}</span>
          <h3 className="text-xl font-semibold text-white">{title}</h3>
        </div>
        <p className="text-neutral-400 leading-relaxed">{description}</p>
      </div>

      {/* Details list */}
      <ul className="space-y-2">
        {details.map((detail, index) => (
          <li key={index} className="flex items-start gap-3 text-sm">
            <span className="text-green-400 mt-0.5 font-bold">→</span>
            <span className="text-neutral-300">{detail}</span>
          </li>
        ))}
      </ul>

      {/* Learn more link */}
      <button className="mt-6 inline-flex items-center gap-2 text-sm text-green-400 hover:text-green-300 transition-colors group-hover:gap-3">
        Learn more about {title.toLowerCase()}
        <svg
          className="w-4 h-4"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          strokeWidth={2}
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
        </svg>
      </button>
    </div>
  );
}

export function WhatsCatchSection() {
  return (
    <section className="relative text-white py-24 overflow-hidden">
      <div className="relative z-10 max-w-7xl mx-auto px-6">
        {/* Header */}
        <div className="text-center mb-16 space-y-4">
          <h2 className="text-4xl md:text-5xl font-bold">
            Okay, so what's the catch?
          </h2>
          <p className="text-neutral-400 text-lg max-w-2xl mx-auto">
            Every tool has tradeoffs. Here's what to expect when adopting iii.
          </p>
        </div>

        {/* Cards grid */}
        <div className="grid md:grid-cols-3 gap-6 mb-12">
          {catchCards.map((card, index) => (
            <CatchCard key={index} {...card} />
          ))}
        </div>

        {/* Bottom CTA */}
        <div className="text-center">
          <p className="text-neutral-400 mb-6">
            Don't let these concerns stop you. They're all surmountable with the
            right guidance.
          </p>
          <button className="inline-flex items-center gap-2 bg-green-500 text-black font-semibold px-6 py-3 rounded-lg hover:bg-green-400 transition-colors">
            Read our adoption guide
            <svg
              className="w-4 h-4"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M13 7l5 5m0 0l-5 5m5-5H6"
              />
            </svg>
          </button>
        </div>
      </div>
    </section>
  );
}
