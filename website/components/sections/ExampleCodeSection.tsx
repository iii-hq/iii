import React, { useState } from "react";
import { Highlight, themes } from "prism-react-renderer";
import { Zap } from "lucide-react";
import { codeExamples } from "./CodeExamples";
import { DependencyVisualization } from "./DependencyVisualization";

// Categories showing what iii Engine replaces and enables
const replaceCategories = [
  { id: "api", label: "API" },
  { id: "jobs", label: "Background Jobs" },
  { id: "events", label: "Pub/Sub" },
  { id: "realtime", label: "Realtime" },
  { id: "state", label: "State" },
  { id: "cron", label: "Cron" },
  { id: "logging", label: "Observability" },
  { id: "workflow", label: "Workflows" },
];

const enableCategories = [
  { id: "ai-agents", label: "AI Agents" },
  { id: "feature-flags", label: "Feature Flags" },
  { id: "multiplayer", label: "Multiplayer" },
  { id: "etl", label: "ETL" },
  { id: "reactive", label: "Reactive" },
  { id: "remote", label: "Remote Invoke" },
];

interface ToolBadgeProps {
  tool: string;
  isDarkMode: boolean;
}

const ToolBadge: React.FC<ToolBadgeProps> = ({ tool, isDarkMode }) => {
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded text-[10px] sm:text-xs font-medium transition-colors ${
        isDarkMode
          ? "bg-iii-alert/20 text-iii-alert border border-iii-alert/30"
          : "bg-iii-alert/10 text-iii-alert border border-iii-alert/20"
      }`}
    >
      {tool}
    </span>
  );
};

// Helper function to count actual lines of code (excluding comments, empty lines)
function countLinesOfCode(code: string, language: string): number {
  const lines = code.split("\n");
  let count = 0;
  let inBlockComment = false;

  for (const line of lines) {
    const trimmed = line.trim();

    // Skip empty lines
    if (!trimmed) continue;

    // Handle block comments
    if (language === "python") {
      // Python uses ''' or """ for block comments/docstrings
      if (trimmed.startsWith('"""') || trimmed.startsWith("'''")) {
        if (trimmed.endsWith('"""') || trimmed.endsWith("'''")) {
          // Single-line docstring, skip it
          if (trimmed.length > 3) continue;
        }
        inBlockComment = !inBlockComment;
        continue;
      }
    } else {
      // JS/TS block comments
      if (trimmed.startsWith("/*")) {
        inBlockComment = true;
        if (trimmed.endsWith("*/")) {
          inBlockComment = false;
        }
        continue;
      }
      if (inBlockComment) {
        if (trimmed.endsWith("*/")) {
          inBlockComment = false;
        }
        continue;
      }
    }

    if (inBlockComment) continue;

    // Skip single-line comments
    if (language === "python") {
      if (trimmed.startsWith("#")) continue;
    } else {
      if (trimmed.startsWith("//")) continue;
    }

    // Skip lines that are only braces/brackets
    if (/^[\{\}\[\]\(\)]+$/.test(trimmed)) continue;

    count++;
  }

  return count;
}

function CodeBlock({
  code,
  title,
  tools,
  variant,
  isDarkMode,
  language = "typescript",
}: {
  code: string;
  title: string;
  tools?: string[];
  variant: "traditional" | "iii";
  isDarkMode: boolean;
  language?: string;
}) {
  const isTraditional = variant === "traditional";
  const lineCount = countLinesOfCode(code, language);

  return (
    <div
      className={`rounded-lg overflow-hidden border h-full flex flex-col transition-colors duration-300 ${
        isDarkMode
          ? "border-iii-light bg-iii-black"
          : "border-iii-dark bg-white"
      }`}
    >
      {/* Header */}
      <div
        className={`flex flex-col gap-2 px-3 sm:px-4 py-2 sm:py-3 border-b transition-colors duration-300 flex-shrink-0 ${
          isDarkMode
            ? "border-iii-light bg-iii-dark/50"
            : "border-iii-dark bg-iii-light/50"
        }`}
      >
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div
              className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${
                isTraditional
                  ? "bg-iii-alert"
                  : isDarkMode
                    ? "bg-iii-accent"
                    : "bg-iii-accent-light"
              }`}
            />
            <span
              className={`text-xs sm:text-sm font-medium transition-colors duration-300 ${
                isDarkMode ? "text-iii-light" : "text-iii-black"
              }`}
            >
              {title}
            </span>
          </div>
          <span
            className={`text-[10px] sm:text-xs px-2 py-0.5 rounded font-medium transition-colors ${
              isTraditional
                ? isDarkMode
                  ? "bg-iii-alert/20 text-iii-alert"
                  : "bg-iii-alert/10 text-iii-alert"
                : isDarkMode
                  ? "bg-iii-accent/20 text-iii-accent"
                  : "bg-iii-accent-light/20 text-iii-accent-light"
            }`}
          >
            {lineCount} lines
          </span>
        </div>
        {tools && tools.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {tools.map((tool) => (
              <ToolBadge key={tool} tool={tool} isDarkMode={isDarkMode} />
            ))}
          </div>
        )}
      </div>

      {/* Code */}
      <div
        className={`p-2 sm:p-3 md:p-4 overflow-auto flex-1 max-h-[400px] sm:max-h-[500px] ${
          isDarkMode ? "scrollbar-brand-dark" : "scrollbar-brand-light"
        }`}
      >
        <Highlight
          key={isDarkMode ? "dark" : "light"}
          theme={isDarkMode ? themes.nightOwl : themes.github}
          code={code.trim()}
          language={language as any}
        >
          {({ tokens, getLineProps, getTokenProps }) => (
            <pre className="text-[10px] sm:text-xs md:text-sm font-mono leading-relaxed overflow-x-auto">
              {tokens.map((line, i) => (
                <div
                  key={i}
                  {...getLineProps({ line })}
                  className="whitespace-pre"
                >
                  <span
                    className={`inline-block w-6 sm:w-8 text-right mr-2 sm:mr-3 select-none ${
                      isDarkMode ? "text-iii-light/30" : "text-iii-medium/40"
                    }`}
                  >
                    {i + 1}
                  </span>
                  {line.map((token, key) => (
                    <span key={key} {...getTokenProps({ token })} />
                  ))}
                </div>
              ))}
            </pre>
          )}
        </Highlight>
      </div>
    </div>
  );
}

function SavingsIndicator({
  traditionalCode,
  iiiCode,
  traditionalLanguage,
  iiiLanguage,
  toolsCount,
  isDarkMode,
}: {
  traditionalCode: string;
  iiiCode: string;
  traditionalLanguage: string;
  iiiLanguage: string;
  toolsCount: number;
  isDarkMode: boolean;
}) {
  const traditional = countLinesOfCode(traditionalCode, traditionalLanguage);
  const iii = countLinesOfCode(iiiCode, iiiLanguage);
  const difference = traditional - iii;
  const isLess = difference > 0;
  const isSame = difference === 0;
  const percentage = Math.round((Math.abs(difference) / traditional) * 100);

  // Different colors for each state
  const codeComparisonColor = isSame
    ? "text-iii-info"
    : isLess
      ? "text-iii-success"
      : "text-iii-warn";

  return (
    <div
      className={`flex flex-col sm:flex-row items-center justify-center gap-3 sm:gap-6 py-3 sm:py-4 px-4 rounded-lg transition-colors ${
        isDarkMode ? "bg-iii-dark/30" : "bg-iii-light"
      }`}
    >
      <div className="flex items-center gap-2">
        {isSame ? (
          // Equals icon for same
          <svg
            className={`w-4 h-4 sm:w-5 sm:h-5 ${codeComparisonColor}`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M5 9h14M5 15h14"
            />
          </svg>
        ) : isLess ? (
          // Downward trend icon for less code (good)
          <svg
            className={`w-4 h-4 sm:w-5 sm:h-5 ${codeComparisonColor}`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M13 17h8m0 0v-8m0 8l-8-8-4 4-6-6"
            />
          </svg>
        ) : (
          // Upward trend icon for more code (warning)
          <svg
            className={`w-4 h-4 sm:w-5 sm:h-5 ${codeComparisonColor}`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6"
            />
          </svg>
        )}
        <span
          className={`text-xs sm:text-sm ${
            isDarkMode ? "text-[#C0C0C0]" : "text-iii-black"
          }`}
        >
          {isSame ? (
            <span className={`font-bold ${codeComparisonColor}`}>
              Same amount of code
            </span>
          ) : (
            <>
              <span className={`font-bold ${codeComparisonColor}`}>
                {percentage}%
              </span>{" "}
              {isLess ? "less" : "more"} code
            </>
          )}
        </span>
      </div>
      <div
        className={`hidden sm:block w-px h-4 ${
          isDarkMode ? "bg-[#3A3A3A]" : "bg-iii-medium/30"
        }`}
      />
      <div className="flex items-center gap-2">
        <svg
          className={`w-4 h-4 sm:w-5 sm:h-5 ${
            isDarkMode ? "text-iii-accent" : "text-iii-accent-light"
          }`}
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
          />
        </svg>
        <span
          className={`text-xs sm:text-sm ${
            isDarkMode ? "text-[#C0C0C0]" : "text-iii-black"
          }`}
        >
          <span
            className={`font-bold ${
              isDarkMode ? "text-iii-accent" : "text-iii-accent-light"
            }`}
          >
            {toolsCount}+
          </span>{" "}
          tools replaced
        </span>
      </div>
      <div
        className={`hidden sm:block w-px h-4 ${
          isDarkMode ? "bg-[#3A3A3A]" : "bg-iii-medium/30"
        }`}
      />
      <div className="flex items-center gap-2">
        <svg
          className={`w-4 h-4 sm:w-5 sm:h-5 ${
            isDarkMode ? "text-iii-accent" : "text-iii-accent-light"
          }`}
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M3.055 11H5a2 2 0 012 2v1a2 2 0 002 2 2 2 0 012 2v2.945M8 3.935V5.5A2.5 2.5 0 0010.5 8h.5a2 2 0 012 2 2 2 0 104 0 2 2 0 012-2h1.064M15 20.488V18a2 2 0 012-2h3.064"
          />
        </svg>
        <span
          className={`text-xs sm:text-sm ${
            isDarkMode ? "text-[#C0C0C0]" : "text-iii-black"
          }`}
        >
          <span
            className={`font-bold ${
              isDarkMode ? "text-iii-accent" : "text-iii-accent-light"
            }`}
          >
            Any
          </span>{" "}
          language
        </span>
      </div>
    </div>
  );
}

interface ExampleCodeSectionProps {
  isDarkMode?: boolean;
}

export function ExampleCodeSection({
  isDarkMode = true,
}: ExampleCodeSectionProps) {
  const [activeCategory, setActiveCategory] = useState("api");

  const currentExample = codeExamples[activeCategory];

  return (
    <section
      className={`relative overflow-hidden font-mono transition-colors duration-300 ${
        isDarkMode ? "text-iii-light" : "text-iii-black"
      }`}
    >
      <div className="relative z-10">
        {/* Header */}
        <div className="text-center mb-8 md:mb-12 space-y-3 md:space-y-4">
          <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full border border-iii-accent/30 bg-iii-accent/5 mb-4">
            <Zap
              className={`w-4 h-4 ${isDarkMode ? "text-iii-accent" : "text-iii-accent-light"}`}
            />
            <span
              className={`text-xs font-mono tracking-wider uppercase ${isDarkMode ? "text-iii-accent" : "text-iii-accent-light"}`}
            >
              Side-by-Side Comparisons
            </span>
          </div>
          <h2 className="text-xl sm:text-2xl md:text-3xl lg:text-4xl xl:text-5xl font-bold tracking-tighter px-2">
            <p>Services, frameworks, integrations,</p>
            <p>all become design patterns.</p>
          </h2>
          <p
            className={`text-xs sm:text-sm md:text-base lg:text-lg max-w-3xl mx-auto px-2 ${
              isDarkMode ? "text-iii-light/70" : "text-iii-medium"
            }`}
          >
            Stop assembling, start building.
          </p>
        </div>

        {/* Category Pills */}
        <div className="mb-4 md:mb-6 space-y-3">
          <div>
            <div
              className={`text-[10px] uppercase tracking-widest mb-2 text-center ${isDarkMode ? "text-iii-light/40" : "text-iii-black/40"}`}
            >
              Replaces
            </div>
            <div className="flex pb-1 justify-center">
              <div className="flex gap-1.5 sm:gap-2 flex-wrap justify-center px-2">
                {replaceCategories.map((category) => (
                  <button
                    key={category.id}
                    onClick={() => setActiveCategory(category.id)}
                    className={`px-3 sm:px-4 py-1.5 sm:py-2 rounded-full text-xs sm:text-sm transition-all whitespace-nowrap font-medium ${
                      activeCategory === category.id
                        ? isDarkMode
                          ? "bg-iii-accent text-iii-black border border-iii-accent"
                          : "bg-iii-accent-light text-iii-light border border-iii-accent-light"
                        : isDarkMode
                          ? "text-iii-light/50 hover:text-iii-light hover:bg-iii-dark/50 border border-iii-light/30"
                          : "text-iii-medium/60 hover:text-iii-black hover:bg-iii-medium/10 border border-iii-dark/30"
                    }`}
                  >
                    {category.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
          <div>
            <div
              className={`text-[10px] uppercase tracking-widest mb-2 text-center ${isDarkMode ? "text-iii-light/40" : "text-iii-black/40"}`}
            >
              Enables
            </div>
            <div className="flex pb-1 justify-center">
              <div className="flex gap-1.5 sm:gap-2 flex-wrap justify-center px-2">
                {enableCategories.map((category) => (
                  <button
                    key={category.id}
                    onClick={() => setActiveCategory(category.id)}
                    className={`px-3 sm:px-4 py-1.5 sm:py-2 rounded-full text-xs sm:text-sm transition-all whitespace-nowrap font-medium ${
                      activeCategory === category.id
                        ? isDarkMode
                          ? "bg-iii-accent text-iii-black border border-iii-accent"
                          : "bg-iii-accent-light text-iii-light border border-iii-accent-light"
                        : isDarkMode
                          ? "text-iii-light/50 hover:text-iii-light hover:bg-iii-dark/50 border border-iii-light/30"
                          : "text-iii-medium/60 hover:text-iii-black hover:bg-iii-medium/10 border border-iii-dark/30"
                    }`}
                  >
                    {category.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Description */}
        {currentExample && (
          <div className="mb-6 md:mb-8 max-w-2xl mx-auto flex items-center justify-center px-2 min-h-[40px] md:min-h-[48px]">
            <p
              className={`text-center text-[11px] sm:text-xs md:text-sm leading-5 sm:leading-6 ${
                isDarkMode ? "text-iii-light/50" : "text-iii-medium/70"
              }`}
            >
              {currentExample.description}
            </p>
          </div>
        )}

        {/* Code comparison with dependency visualization */}
        {currentExample && (
          <div
            key={activeCategory}
            className="animate-fade-slide-in lg:max-w-[95%] lg:mx-auto"
          >
            <DependencyVisualization
              traditionalCode={currentExample.traditional.code}
              traditionalTitle={currentExample.traditional.title}
              traditionalTools={currentExample.traditional.tools}
              traditionalLanguage={currentExample.traditional.language}
              iiiCode={currentExample.iii.code}
              iiiTitle={currentExample.iii.title}
              iiiLanguage={currentExample.iii.language}
              categoryId={activeCategory}
              isDarkMode={isDarkMode}
            />
          </div>
        )}
      </div>
    </section>
  );
}
