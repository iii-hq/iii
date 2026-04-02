import { useState } from "react";
import { HeroSection } from "../components/sections/HeroSection";
import { ExampleCodeSection } from "../components/sections/ExampleCodeSection";
import { TestimonialsSection } from "../components/sections/TestimonialsSection";
import { WhatsCatchSection } from "../components/sections/WhatsCatchSection";
import { FooterSection } from "../components/sections/FooterSection";
import {
  DependencyStack,
  DependencyStackBackground,
  DependencyStackCorner,
  DependencyStackSplash,
} from "../components/DependencyStack";

export function SectionsPreview() {
  return (
    <div className="min-h-screen bg-black relative">
      {/* Scroll-animated dependency stack background */}
      <DependencyStackBackground />

      {/* Corner variant - bottom left */}
      <DependencyStackCorner />

      {/* Navigation for preview */}
      <nav className="fixed top-0 left-0 right-0 z-50 bg-black/80 backdrop-blur border-b border-neutral-800">
        <div className="max-w-7xl mx-auto px-6 py-3 flex items-center justify-between">
          <a href="/" className="text-white text-sm hover:text-neutral-300">
            ← Back to main site
          </a>
          <span className="text-neutral-500 text-sm">iii Preview</span>
        </div>
      </nav>

      {/* Section components */}
      <div className="pt-12 relative z-10">
        <HeroSection />
        {/* <PersonaValueProps /> */}
        {/* <CodeComparisonSection /> */}
        <ExampleCodeSection />
        <TestimonialsSection />
        <WhatsCatchSection />
        <FooterSection />

        {/* Splash Demo Section */}
        <section className="py-24 px-6">
          <div className="max-w-4xl mx-auto">
            <h2 className="text-3xl font-bold text-white text-center mb-4">
              Dependency Stack Splash Demo
            </h2>
            <p className="text-neutral-400 text-center mb-12">
              The same component configured as a centered splash element
            </p>
            <div className="bg-neutral-900/50 rounded-2xl border border-neutral-800 h-[500px] relative overflow-hidden">
              <DependencyStackSplash
                animateOnScroll={false}
                progress={0}
                opacity={0.3}
              />
              <div className="absolute inset-0 flex items-center justify-center">
                <div className="text-center z-10">
                  <h3 className="text-2xl font-bold text-white mb-2">
                    Your Product Here
                  </h3>
                  <p className="text-neutral-400">
                    Overlay content on top of the animation
                  </p>
                </div>
              </div>
            </div>
          </div>
        </section>

        {/* Interactive Demo Section */}
        <DependencyStackDemo />
      </div>
    </div>
  );
}

/** Interactive demo to control the animation progress manually */
function DependencyStackDemo() {
  const [progress, setProgress] = useState(0);

  return (
    <section className="py-24 px-6 bg-neutral-950">
      <div className="max-w-4xl mx-auto">
        <h2 className="text-3xl font-bold text-white text-center mb-4">
          Interactive Demo
        </h2>
        <p className="text-neutral-400 text-center mb-8">
          Drag the slider to manually control the animation
        </p>

        <div className="mb-8">
          <input
            type="range"
            min="0"
            max="100"
            value={progress * 100}
            onChange={(e) => setProgress(Number(e.target.value) / 100)}
            className="w-full h-2 bg-neutral-800 rounded-lg appearance-none cursor-pointer accent-iii-alert"
          />
          <div className="flex justify-between text-sm text-neutral-500 mt-2">
            <span>Stack (0%)</span>
            <span>Progress: {Math.round(progress * 100)}%</span>
            <span>Grid (100%)</span>
          </div>
        </div>

        <div className="bg-neutral-900/50 rounded-2xl border border-neutral-800 h-[600px] relative overflow-hidden">
          <DependencyStack
            variant="splash"
            animateOnScroll={false}
            progress={progress}
            opacity={0.4}
            showBracket={true}
          />
        </div>
      </div>
    </section>
  );
}
