import { useEffect, useMemo, useRef, useState } from "react";
import { Highlight, themes } from "prism-react-renderer";
import type { CodeEditorStep } from "./types";

interface CodeEditorProps {
  step: CodeEditorStep;
  isActive: boolean;
  onNext: () => void;
}

export function CodeEditor({ step, isActive, onNext }: CodeEditorProps) {
  const [visibleLines, setVisibleLines] = useState(0);
  const [isWriting, setIsWriting] = useState(false);
  const [writeDone, setWriteDone] = useState(false);
  const [completed, setCompleted] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const codeScrollRef = useRef<HTMLDivElement>(null);

  const lineDelay = step.lineDelay ?? 220;
  const codeToHighlight = useMemo(
    () => step.lines.slice(0, visibleLines).join("\n"),
    [step.lines, visibleLines],
  );

  const handleWrite = () => {
    if (isWriting || writeDone) return;
    setIsWriting(true);
    let lineIndex = 0;

    intervalRef.current = setInterval(() => {
      lineIndex += 1;
      setVisibleLines(lineIndex);

      if (lineIndex >= step.lines.length) {
        if (intervalRef.current) clearInterval(intervalRef.current);
        setIsWriting(false);
        setWriteDone(true);
      }
    }, lineDelay);
  };

  const handleDone = () => {
    if (completed) return;
    setCompleted(true);
    onNext();
  };

  useEffect(() => {
    if (!isActive || isWriting || writeDone) return;
    const timer = setTimeout(handleWrite, 400);
    return () => clearTimeout(timer);
  }, [isActive, isWriting, writeDone]);

  useEffect(() => {
    if (!codeScrollRef.current) return;
    codeScrollRef.current.scrollTo({
      top: codeScrollRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [visibleLines]);

  useEffect(() => {
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  if (!isActive && completed) {
    return (
      <div className="rounded-lg border border-iii-success/20 bg-iii-success/5 px-3 py-2.5">
        <p className="text-xs text-iii-success font-bold">
          ✓ {step.filename} completed
        </p>
      </div>
    );
  }

  return (
    <div
      className={`rounded-lg border border-iii-medium/20 bg-iii-dark overflow-hidden ${
        isActive
          ? "animate-[fadeSlideIn_0.3s_ease-out_forwards] opacity-0"
          : "opacity-75"
      }`}
      style={isActive ? { animationFillMode: "forwards" } : undefined}
    >
      <div className="flex items-center justify-between px-3 py-1.5 bg-iii-black/50 border-b border-iii-medium/10">
        <div className="flex items-center gap-2">
          <div className="flex gap-1">
            <span className="w-2 h-2 rounded-full bg-iii-alert/60" />
            <span className="w-2 h-2 rounded-full bg-iii-warn/60" />
            <span className="w-2 h-2 rounded-full bg-iii-success/60" />
          </div>
          <span className="text-[11px] text-iii-medium font-mono">{step.filename}</span>
        </div>
      </div>

      <div ref={codeScrollRef} className="px-0 py-3 min-h-[80px] max-h-[300px] overflow-y-auto scrollbar-brand-dark">
        {visibleLines === 0 && !isWriting ? (
          <div className="flex items-center justify-center py-6">
            <span className="text-xs text-iii-medium/40 italic">Ready to write...</span>
          </div>
        ) : (
          <Highlight
            code={codeToHighlight || " "}
            language={step.language as any}
            theme={themes.palenight}
          >
            {({ style, tokens, getLineProps, getTokenProps }) => (
              <pre
                className="text-xs leading-relaxed font-mono"
                style={{ ...style, background: "transparent", margin: 0, padding: "0 12px" }}
              >
                {tokens.map((line, i) => {
                  const lineProps = getLineProps({ line, key: i });
                  return (
                    <div
                      key={i}
                      {...lineProps}
                      className={`flex ${lineProps.className ?? ""}`}
                      style={{
                        ...lineProps.style,
                        animation: "fadeSlideIn 0.2s ease-out forwards",
                        opacity: 0,
                        animationDelay: `${i * 30}ms`,
                      }}
                    >
                      <span className="select-none text-iii-medium/30 w-8 text-right mr-4 shrink-0">
                        {i + 1}
                      </span>
                      <span>
                        {line.map((token, key) => (
                          <span key={key} {...getTokenProps({ token, key })} />
                        ))}
                      </span>
                    </div>
                  );
                })}
                {isWriting && (
                  <div className="flex">
                    <span className="select-none text-iii-medium/30 w-8 text-right mr-4 shrink-0">
                      {visibleLines + 1}
                    </span>
                    <span className="inline-block w-[2px] h-[14px] bg-iii-accent animate-pulse" />
                  </div>
                )}
              </pre>
            )}
          </Highlight>
        )}
      </div>

      {isActive && (
        <div className="flex justify-end px-3 py-2 border-t border-iii-medium/10">
          {isWriting && (
            <span className="px-4 py-1.5 text-xs text-iii-medium/60 italic">writing...</span>
          )}
          {writeDone && !completed && (
            <button
              onClick={handleDone}
              className="px-4 py-1.5 text-xs font-bold rounded bg-iii-accent text-iii-black hover:brightness-110 transition-all duration-200 cursor-pointer animate-[fadeSlideIn_0.2s_ease-out_forwards]"
            >
              {step.doneLabel ?? "Deploy"}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
