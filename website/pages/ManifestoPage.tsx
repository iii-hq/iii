import React, { useState, useEffect, useCallback } from "react";
import { Zap, Network, Shield, GitBranch, Repeat } from "lucide-react";
import {
  GlobeIcon,
  CpuIcon,
  MagnifierIcon,
  EyeIcon,
  BrainCircuitIcon,
} from "../components/icons";
import { Navbar } from "../components/Navbar";
import { Terminal } from "../components/Terminal";
import { KeySequence } from "../types";

const shifts = [
  {
    icon: GlobeIcon,
    title: "POLYGLOT",
    problem:
      "Each language needs its own framework. Each deployment location needs its own infrastructure. The mental overhead compounds exponentially.",
    future:
      "Language agnostic by design, not by accident. Node.js, Python, Rust, Go, browser, edge, embedded — all connect via the same protocol. A basement GPU and a Cloudflare Worker are peers in the same mesh.",
  },
  {
    icon: CpuIcon,
    title: "SMALL SURFACE AREA OF PRIMITIVES",
    problem:
      "Domain-specific frameworks multiply. Queue adapters, API gateways, service meshes, workflow engines — each pattern demands its own tooling, its own mental model, its own failure modes.",
    future:
      "A small surface area of core primitives that compose infinitely. These primitives handle ALL backend patterns that used to be solved by domain specific frameworks and tools. One kernel, infinite compositions.",
  },
  {
    icon: Network,
    title: "UNIVERSAL ACCESSIBILITY",
    problem:
      "Legacy servers can't talk to edge functions. Serverless can't reach embedded devices. Each integration is a custom bridge, a maintenance burden, a potential failure point.",
    future:
      "Every dependency and integration accessible to every service. Legacy servers, edge functions, serverless, embedded devices — all first-class participants in the same unified mesh.",
  },
  {
    icon: Shield,
    title: "SECURE BIDIRECTIONAL COMMUNICATION",
    problem:
      "Services communicate through fragile, one-way channels. Request-response patterns dominate. Real-time sync requires separate infrastructure.",
    future:
      "Secure bidirectional communication between all services by default. Every service can push and pull. Every connection is encrypted. Trust is built into the protocol.",
  },
  {
    icon: Zap,
    title: "DYNAMIC REGISTRATION",
    problem:
      "gRPC requires IDL files. REST needs OpenAPI specs. Service meshes demand sidecar configs. Every approach forces static definitions before a single function can be called.",
    future:
      "Dynamic registration at runtime. Workers connect, register functions, and they're immediately available. No compilation, no code generation, no spec files.",
  },
  {
    icon: MagnifierIcon,
    title: "SELF DISCOVERABLE",
    problem:
      "Service discovery is a separate system. Consul, etcd, Kubernetes DNS — another layer to configure, another thing that can fail, another mental model to learn.",
    future:
      "The mesh is self-discoverable. It knows what exists and how to reach it. Services find each other automatically. No external discovery layer required.",
  },
  {
    icon: EyeIcon,
    title: "OBSERVABLE & TRACEABLE BY DEFAULT",
    problem:
      "Observability is bolted on. Tracing requires instrumentation. Metrics need exporters. Logs are scattered across systems. Understanding your system requires assembling a dozen tools.",
    future:
      "Tracing, metrics, and logging built into the protocol. Every invocation is observable. Every transaction is traceable. Understanding the system is the default, not an afterthought.",
  },
  {
    icon: GitBranch,
    title: "POLYMORPHIC TRIGGERS",
    problem:
      "HTTP handlers, queue consumers, cron jobs, event listeners — each trigger type demands different infrastructure, different deployment patterns, different debugging approaches.",
    future:
      "Composable with any event source. HTTP, state updates, gRPC, cron, events, hardware interrupts — all normalize to the same invocation model. One function, infinite triggers.",
  },
  {
    icon: BrainCircuitIcon,
    title: "AGENT-FIRST ARCHITECTURE",
    problem:
      "AI agents require separate tool definitions, manual integration, and constant synchronization. When agents change paths mid-execution, systems break.",
    future:
      "Maximize the surface area for agent success, even when they change paths. Functions self-describe with schemas and semantic metadata. The system adapts to agent behavior, not vice versa.",
  },
  {
    icon: Repeat,
    title: "REVERSIBLE TRANSACTIONS",
    problem:
      "Distributed transactions are fire-and-forget. When something fails mid-chain, recovery is manual. Debugging requires reconstructing state from scattered logs.",
    future:
      "Every transaction chain is replayable, modifiable, and reversible. Debug by replaying. Recover by rewinding. The past is not just logged — it's executable.",
  },
];

export const ManifestoPage: React.FC = () => {
  const [isDarkMode, setIsDarkMode] = useState(true);
  const [isHumanMode] = useState(true);
  const [logoClickCount, setLogoClickCount] = useState(0);
  const [isLogoHovered, setIsLogoHovered] = useState(false);
  const [hoverAnimIndex, setHoverAnimIndex] = useState(-1);
  const [showTerminal, setShowTerminal] = useState(false);
  const [isGodMode, setIsGodMode] = useState(false);
  const [showGodModeUnlock, setShowGodModeUnlock] = useState(false);

  const bgColor = isDarkMode ? "bg-iii-black" : "bg-iii-light";
  const textPrimary = isDarkMode ? "text-iii-light" : "text-iii-black";
  const textSecondary = isDarkMode ? "text-iii-light/70" : "text-iii-black/70";
  const accentColor = isDarkMode ? "text-iii-accent" : "text-iii-accent-light";
  const accentBg = isDarkMode ? "bg-iii-accent/5" : "bg-iii-accent-light/5";
  const accentBorder = isDarkMode
    ? "border-iii-accent/20"
    : "border-iii-accent-light/20";
  const sectionBorder = isDarkMode
    ? "border-iii-light/10"
    : "border-iii-black/10";
  const dimText = isDarkMode ? "text-iii-light/50" : "text-iii-black/50";
  const borderColor = isDarkMode
    ? "border-iii-light/20"
    : "border-iii-black/20";

  const toggleTheme = () => setIsDarkMode(!isDarkMode);
  const toggleMode = () => {
    window.location.href = "/ai";
  };

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

  const handleLogoClick = useCallback(() => {
    const newCount = logoClickCount + 1;
    if (newCount >= 3) {
      setLogoClickCount(3);
      setTimeout(() => {
        setShowTerminal(true);
        setLogoClickCount(0);
      }, 300);
    } else {
      setLogoClickCount(newCount);
    }
  }, [logoClickCount]);

  useEffect(() => {
    let keyBuffer: string[] = [];
    const handleKeyDown = (e: KeyboardEvent) => {
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      keyBuffer.push(key);
      if (keyBuffer.length > 30) keyBuffer = keyBuffer.slice(-30);
      const recentKeys = keyBuffer.slice(-3).join("");
      if (recentKeys === KeySequence.III) setShowTerminal(true);
      const fullHistory = keyBuffer.join("");
      if (fullHistory.includes(KeySequence.KONAMI)) {
        setIsGodMode(true);
        setShowGodModeUnlock(true);
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

  return (
    <div className={`min-h-screen ${bgColor} ${textPrimary} font-mono`}>
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

      <main className="max-w-4xl mx-auto px-4 sm:px-6 pt-24 md:pt-32 pb-12 md:pb-16 lg:pb-24">
        <div className="text-center mb-12 md:mb-16">
          <div
            className={`inline-flex items-center gap-2 px-4 py-2 rounded-full border ${accentBorder} ${accentBg} mb-6`}
          >
            <span className={`text-lg font-black ${accentColor}`}>iii</span>
            <span className={`text-xs font-mono tracking-wider ${accentColor}`}>
              MANIFESTO
            </span>
          </div>
          <h1 className="text-3xl sm:text-4xl md:text-5xl lg:text-6xl font-bold tracking-tighter mb-6">
            The Future of
            <br />
            <span className={accentColor}>Backend Engineering</span>
          </h1>
          <a
            href="/"
            className={`inline-flex items-center gap-2 px-6 py-3 rounded-lg font-bold text-sm transition-colors ${
              isDarkMode
                ? "bg-iii-accent text-iii-black hover:bg-iii-accent/90"
                : "bg-iii-accent-light text-white hover:bg-iii-accent-light/90"
            }`}
          >
            Try iii
          </a>
        </div>

        <div className="space-y-6 mb-16">
          <p
            className={`text-base md:text-lg leading-relaxed ${textSecondary}`}
          >
            The future of backend engineering demands a new foundation. Not
            another framework. Not another service. A{" "}
            <span className={textPrimary}>universal execution kernel</span>{" "}
            built on primitives that compose infinitely.
          </p>
          <p
            className={`text-base md:text-lg leading-relaxed italic ${dimText}`}
          >
            We are writing integration code when we should be writing business
            logic.
          </p>
          <p
            className={`text-base md:text-lg leading-relaxed ${textSecondary}`}
          >
            <span className={accentColor}>iii</span> is the ideal environment
            for production-grade agents — polyglot, observable, and designed for
            systems that must adapt when agents change paths.
          </p>
        </div>

        <div className={`pt-8 border-t ${sectionBorder}`}>
          <p
            className={`text-xs tracking-[0.2em] uppercase mb-8 ${textSecondary}`}
          >
            The future of backend engineering must be:
          </p>

          <div className="space-y-8">
            {shifts.map((shift, index) => (
              <div
                key={index}
                className={`p-6 md:p-8 border ${sectionBorder} rounded-lg space-y-4`}
              >
                <div className="flex items-center gap-3">
                  <shift.icon size={20} className={`${accentColor} shrink-0`} />
                  <span
                    className={`text-sm md:text-base font-bold tracking-wide ${textPrimary}`}
                  >
                    {index + 1}. {shift.title}
                  </span>
                </div>
                <p
                  className={`text-sm md:text-base leading-relaxed ${dimText}`}
                >
                  {shift.problem}
                </p>
                <p
                  className={`text-sm md:text-base leading-relaxed ${textSecondary}`}
                >
                  <span className={accentColor}>→</span> {shift.future}
                </p>
              </div>
            ))}
          </div>
        </div>

        <div
          className={`mt-16 p-6 md:p-8 border-2 ${accentBorder} ${accentBg} rounded-lg`}
        >
          <p className={`text-lg md:text-xl font-bold ${textPrimary} mb-4`}>
            THE COMMITMENT
          </p>
          <p
            className={`text-base md:text-lg leading-relaxed ${textSecondary} mb-6`}
          >
            iii treats everything as a first-class worker — legacy servers, edge
            functions, serverless, embedded devices. Every transaction chain is
            replayable, modifiable, and reversible. The mesh is
            self-discoverable, observable by default, and built for agents that
            succeed even when they change paths.
          </p>
          <p className={`text-xl md:text-2xl ${accentColor} font-bold mb-6`}>
            One Binary. Infinite Systems.
          </p>
          <a
            href="/"
            className={`inline-flex items-center gap-2 px-6 py-3 rounded-lg font-bold text-sm transition-colors ${
              isDarkMode
                ? "bg-iii-accent text-iii-black hover:bg-iii-accent/90"
                : "bg-iii-accent-light text-white hover:bg-iii-accent-light/90"
            }`}
          >
            Try iii
          </a>
        </div>

        <p
          className={`text-center text-sm tracking-[0.2em] uppercase mt-16 ${textSecondary}`}
        >
          iii / The Universal Execution Kernel
        </p>
      </main>

      <footer className={`border-t ${borderColor} py-8`}>
        <div className="max-w-4xl mx-auto px-4 sm:px-6 text-center">
          <p className={`text-xs ${dimText}`}>© 2026 Motia LLC</p>
        </div>
      </footer>

      {showGodModeUnlock && (
        <div className="fixed inset-0 z-50 bg-black flex items-center justify-center">
          <div className="text-center animate-pulse">
            <div className="text-6xl md:text-8xl font-black text-red-500 mb-4 tracking-tighter animate-bounce">
              GOD MODE
            </div>
            <div className="text-xl md:text-2xl text-red-400 font-mono tracking-widest">
              UNLOCKED
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
    </div>
  );
};
