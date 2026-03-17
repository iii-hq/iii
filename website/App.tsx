import React, { useState, useEffect, useCallback } from "react";
import { Terminal } from "./components/Terminal";
import { ExampleCodeSection } from "./components/sections/ExampleCodeSection";
import { HeroSection } from "./components/sections/HeroSection";
import { InteractiveDemoFlowSection } from "./components/sections/InteractiveDemoFlowSection";
import { HelloWorldSection } from "./components/sections/HelloWorldSection";
import { EngineSection } from "./components/sections/EngineSection";
import { AgentReadySection } from "./components/sections/AgentReadySection";
import { ObservabilitySection } from "./components/sections/ObservabilitySection";
import { FooterSection } from "./components/sections/FooterSection";
import { Navbar } from "./components/Navbar";
import { MachineView } from "./components/MachineView";
import { SectionsPreview } from "./pages/SectionsPreview";
import { ManifestoPage } from "./pages/ManifestoPage";
import { CookieConsent } from "./components/CookieConsent";
import { useCookieConsent } from "./lib/useCookieConsent";
import { KeySequence } from "./types";

const AppRouter: React.FC = () => {
  const pathname = window.location.pathname;

  if (pathname === "/preview") {
    return <SectionsPreview />;
  }
  if (pathname === "/manifesto") {
    return <ManifestoPage />;
  }

  return <App />;
};

const App: React.FC = () => {
  const { consent, accept, reject } = useCookieConsent();
  const [showTerminal, setShowTerminal] = useState(false);
  const [isGodMode, setIsGodMode] = useState(false);
  const [isDarkMode, setIsDarkMode] = useState(true);
  const [logoClickCount, setLogoClickCount] = useState(0);
  const [isLogoHovered, setIsLogoHovered] = useState(false);
  const [hoverAnimIndex, setHoverAnimIndex] = useState(-1);
  const [showGodModeUnlock, setShowGodModeUnlock] = useState(false);

  // URL-based mode detection: /ai = machine mode, / = human mode
  const [isHumanMode, setIsHumanMode] = useState(() => {
    return window.location.pathname !== "/ai";
  });

  useEffect(() => {
    document.body.classList.toggle("dark-mode", isDarkMode);
    document.body.classList.toggle("light-mode", !isDarkMode);
  }, [isDarkMode]);

  const toggleTheme = () => setIsDarkMode(!isDarkMode);

  const toggleMode = () => {
    const newMode = !isHumanMode;
    setIsHumanMode(newMode);
    // Update URL without reload
    const newPath = newMode ? "/" : "/ai";
    window.history.pushState({}, "", newPath);
  };

  // Handle browser back/forward
  useEffect(() => {
    const handlePopState = () => {
      setIsHumanMode(window.location.pathname !== "/ai");
    };
    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  }, []);

  // Hover animation - sequential highlight
  useEffect(() => {
    if (!isLogoHovered || logoClickCount > 0) {
      setHoverAnimIndex(-1);
      return;
    }

    let index = 0;
    const interval = setInterval(() => {
      setHoverAnimIndex(index % 3);
      index++;
    }, 200);

    return () => clearInterval(interval);
  }, [isLogoHovered, logoClickCount]);

  // Click handler - each click locks another "i"
  const handleLogoClick = useCallback(() => {
    const newCount = logoClickCount + 1;

    if (newCount >= 3) {
      // Three clicks - all lit, open terminal
      setLogoClickCount(3);
      setTimeout(() => {
        setShowTerminal(true);
        setLogoClickCount(0); // Reset after opening
      }, 300);
    } else {
      setLogoClickCount(newCount);
      // Clicks now hold forever until terminal opens
    }
  }, [logoClickCount]);

  useEffect(() => {
    let keyBuffer: string[] = [];

    const handleKeyDown = (e: KeyboardEvent) => {
      // Normalize key - lowercase for letters, keep arrow keys as-is
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      keyBuffer.push(key);
      if (keyBuffer.length > 30) keyBuffer = keyBuffer.slice(-30);

      const recentKeys = keyBuffer.slice(-3).join("");
      if (recentKeys === KeySequence.III) setShowTerminal(true);

      const fullHistory = keyBuffer.join("");
      if (fullHistory.includes(KeySequence.KONAMI)) {
        setIsGodMode(true);
        setShowGodModeUnlock(true);
        // Show unlock animation then open terminal
        setTimeout(() => {
          setShowGodModeUnlock(false);
          setShowTerminal(true);
        }, 2000);
        keyBuffer = [];
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  // Machine mode - raw markdown/text dump for AI consumption
  // Easter eggs still work: Konami code, terminal access
  if (!isHumanMode) {
    return (
      <>
        <MachineView
          onToggleMode={toggleMode}
          onToggleTheme={toggleTheme}
          isGodMode={isGodMode}
          isDarkMode={isDarkMode}
          onLogoClick={handleLogoClick}
        />
        {showTerminal && (
          <Terminal
            onClose={() => setShowTerminal(false)}
            isGodMode={isGodMode}
          />
        )}
        {showGodModeUnlock && (
          <div className="fixed inset-0 z-50 bg-black flex items-center justify-center">
            <div className="text-center animate-pulse">
              <div className="text-6xl md:text-8xl font-black text-red-500 mb-4 tracking-tighter animate-bounce">
                GOD MODE
              </div>
              <div className="text-xl md:text-2xl text-red-400 font-mono tracking-widest">
                UNLOCKED
              </div>
              <div className="mt-8 text-sm text-red-500/50 font-mono">
                ↑↑↓↓←→←→BA
              </div>
            </div>
          </div>
        )}
        {consent === null && (
          <CookieConsent onAccept={accept} onReject={reject} />
        )}
      </>
    );
  }

  return (
    <div
      className={`min-h-screen font-mono selection:bg-iii-accent selection:text-iii-black relative flex flex-col transition-colors duration-300 ${
        isDarkMode
          ? "bg-iii-black text-iii-light"
          : "bg-iii-light text-iii-black"
      } ${isGodMode ? "selection:bg-red-500" : ""}`}
    >
      <Navbar
        isDarkMode={isDarkMode}
        isGodMode={isGodMode}
        isHumanMode={isHumanMode}
        onToggleTheme={toggleTheme}
        onToggleMode={toggleMode}
        onLogoClick={handleLogoClick}
        logoClickCount={logoClickCount}
        isLogoHovered={isLogoHovered}
        hoverAnimIndex={hoverAnimIndex}
        onLogoMouseEnter={() => setIsLogoHovered(true)}
        onLogoMouseLeave={() => setIsLogoHovered(false)}
      />

      <main className="flex-1 relative z-10 flex flex-col items-center w-full pt-16 md:pt-20">
        {/* Section 1: Hero - One Engine. Any Language. Anywhere. */}
        <div className="w-full">
          <HeroSection isDarkMode={isDarkMode} />
        </div>

        <div className="w-full">
          <InteractiveDemoFlowSection isDarkMode={isDarkMode} />
        </div>

        {/* Section 2: Hello World - Polyglot proof with IPC */}
        <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl py-8 md:py-12 lg:py-24">
          <HelloWorldSection isDarkMode={isDarkMode} />
        </div>

        {/* Section 3: Architecture (formerly Engine) - Trigger → Function → Workers */}
        <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl py-8 md:py-12 lg:py-24">
          <EngineSection isDarkMode={isDarkMode} />
        </div>

        {/* Section 4: Triggers as Universal Adapters - Code Examples */}
        <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl py-8 md:py-12 lg:py-24">
          <ExampleCodeSection isDarkMode={isDarkMode} />
        </div>

        {/* Section 5: Observability - Trace-level visibility with OpenTelemetry */}
        <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl py-8 md:py-12 lg:py-24">
          <ObservabilitySection isDarkMode={isDarkMode} />
        </div>

        {/* Section 6: Agent-Ready - AI agents as first-class citizens */}
        <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl py-8 md:py-12 lg:py-24">
          <AgentReadySection isDarkMode={isDarkMode} />
        </div>

        {/* Section 7: Footer + CTA - FAQ, Discord, Links */}
        <div className="w-[95%] md:w-[90%] lg:w-[85%] max-w-7xl py-8 md:py-12 lg:py-24">
          <FooterSection isDarkMode={isDarkMode} />
        </div>
      </main>

      {/* God Mode Unlock Animation */}
      {showGodModeUnlock && (
        <div className="fixed inset-0 z-50 bg-black flex items-center justify-center">
          <div className="text-center animate-pulse">
            <div className="text-6xl md:text-8xl font-black text-red-500 mb-4 tracking-tighter animate-bounce">
              GOD MODE
            </div>
            <div className="text-xl md:text-2xl text-red-400 font-mono tracking-widest">
              UNLOCKED
            </div>
            <div className="mt-8 text-sm text-red-500/50 font-mono">
              ↑↑↓↓←→←→BA
            </div>
            <div className="mt-4 flex justify-center gap-2">
              {[0, 1, 2].map((i) => (
                <div
                  key={i}
                  className="w-3 h-3 bg-red-500 rounded-full animate-ping"
                  style={{ animationDelay: `${i * 0.2}s` }}
                />
              ))}
            </div>
          </div>
        </div>
      )}

      {showTerminal && (
        <Terminal
          onClose={() => setShowTerminal(false)}
          isGodMode={isGodMode}
        />
      )}

      {consent === null && (
        <CookieConsent onAccept={accept} onReject={reject} />
      )}
    </div>
  );
};

export default AppRouter;
