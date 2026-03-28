import React, { useEffect, useState } from "react";
import { Navbar } from "./Navbar";

interface MachineViewProps {
  onToggleMode: () => void;
  onToggleTheme: () => void;
  onOpenTerminal?: () => void;
  isGodMode: boolean;
  isDarkMode?: boolean;
}

export const MachineView: React.FC<MachineViewProps> = ({
  onToggleMode,
  onToggleTheme,
  isGodMode,
  isDarkMode = true,
}) => {
  const [machineMarkdown, setMachineMarkdown] = useState(
    "# iii Homepage for AI\n\nLoading static machine markdown...",
  );

  useEffect(() => {
    const fallbackMarkdown = `# iii Homepage for AI

Unable to load pre-built machine markdown in this session.

## Code Examples
Code examples are intentionally omitted from this machine page.
For implementation details and source examples, visit:
- https://iii.dev/docs
- https://github.com/iii-hq/skills`;

    let cancelled = false;

    const loadStaticMarkdown = async () => {
      try {
        const response = await fetch("/ai.md");
        if (!response.ok) {
          throw new Error(`Failed to load /ai.md (${response.status})`);
        }
        const markdown = await response.text();
        if (!cancelled) {
          setMachineMarkdown(markdown || fallbackMarkdown);
        }
      } catch {
        if (!cancelled) {
          setMachineMarkdown(fallbackMarkdown);
        }
      }
    };
    loadStaticMarkdown();

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div
      className={`min-h-screen font-mono relative flex flex-col transition-colors duration-300 ${
        isDarkMode
          ? "bg-iii-black text-iii-light"
          : "bg-iii-light text-iii-black"
      }`}
    >
      <Navbar
        isDarkMode={isDarkMode}
        isGodMode={isGodMode}
        isHumanMode={false}
        onToggleTheme={onToggleTheme}
        onToggleMode={onToggleMode}
      />

      <div className="flex-1 text-xs md:text-sm leading-relaxed px-4 md:px-8 lg:px-12 pt-24 md:pt-32 lg:pt-32 pb-8 overflow-x-hidden">
        <div className="max-w-4xl mx-auto space-y-4 md:space-y-6 break-words">
          <pre className="whitespace-pre-wrap break-words overflow-x-auto">
            {machineMarkdown}
          </pre>
        </div>
      </div>
    </div>
  );
};
