import React, { useEffect, useState } from "react";
import { generateHomepageMarkdownFromDocument } from "../lib/machineMarkdown";
import { Navbar } from "./Navbar";

interface MachineViewProps {
  onToggleMode: () => void;
  onToggleTheme: () => void;
  onOpenTerminal?: () => void;
  isGodMode: boolean;
  isDarkMode?: boolean;
  onLogoClick?: () => void;
}

export const MachineView: React.FC<MachineViewProps> = ({
  onToggleMode,
  onToggleTheme,
  isGodMode,
  isDarkMode = true,
  onLogoClick,
}) => {
  const [isLogoHovered, setIsLogoHovered] = useState(false);
  const [machineMarkdown, setMachineMarkdown] = useState(
    "# iii Homepage (Auto-generated for AI)\n\nGenerating markdown from homepage...",
  );

  useEffect(() => {
    const fallbackMarkdown = `# iii Homepage (Auto-generated for AI)

Unable to parse homepage content in this session.

## Code Examples
Code examples are intentionally omitted from this machine page.
For implementation details and source examples, visit:
- https://iii.dev/docs
- https://github.com/iii-hq/skills`;

    let cancelled = false;
    let retryTimer: number | null = null;
    const iframe = document.createElement("iframe");
    iframe.src = "/";
    iframe.setAttribute("aria-hidden", "true");
    iframe.tabIndex = -1;
    iframe.style.position = "fixed";
    iframe.style.left = "-20000px";
    iframe.style.top = "0";
    iframe.style.opacity = "0";
    iframe.style.pointerEvents = "none";
    iframe.style.width = "1280px";
    iframe.style.height = "4000px";
    iframe.style.border = "0";

    const attemptExtraction = (attempt = 0) => {
      if (cancelled) {
        return;
      }
      const doc = iframe.contentDocument;
      const hasSections = doc?.querySelectorAll("[data-machine-section]").length;

      if ((!doc || !hasSections) && attempt < 20) {
        retryTimer = window.setTimeout(() => {
          attemptExtraction(attempt + 1);
        }, 150);
        return;
      }

      if (!doc) {
        setMachineMarkdown(fallbackMarkdown);
        return;
      }

      const markdown = generateHomepageMarkdownFromDocument(doc);
      setMachineMarkdown(markdown || fallbackMarkdown);
    };

    const onLoad = () => {
      attemptExtraction();
    };

    iframe.addEventListener("load", onLoad);
    document.body.appendChild(iframe);

    return () => {
      cancelled = true;
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer);
      }
      iframe.removeEventListener("load", onLoad);
      iframe.remove();
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
        onLogoClick={onLogoClick}
        onLogoMouseEnter={() => setIsLogoHovered(true)}
        onLogoMouseLeave={() => setIsLogoHovered(false)}
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
