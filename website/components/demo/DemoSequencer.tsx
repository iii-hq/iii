import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DemoSequencerProps, Step } from "./types";
import { SlackMessage } from "./SlackMessage";
import { ReplyComposer } from "./ReplyComposer";
import { CodeEditor } from "./CodeEditor";
import { StatusPanel } from "./StatusPanel";

export function DemoSequencer({
  steps,
  mode = "hero",
  onComplete,
  className = "",
}: DemoSequencerProps) {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [visibleUpTo, setVisibleUpTo] = useState(-1);
  const [isDelaying, setIsDelaying] = useState(false);
  const [sessionKey, setSessionKey] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const delayRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const isFinished = currentIndex >= steps.length;
  const activeStep = useMemo<Step | undefined>(
    () => steps[currentIndex],
    [steps, currentIndex],
  );

  const clearDelay = useCallback(() => {
    if (!delayRef.current) return;
    clearTimeout(delayRef.current);
    delayRef.current = null;
  }, []);

  const next = useCallback(() => {
    const nextIndex = currentIndex + 1;

    if (nextIndex >= steps.length) {
      setCurrentIndex(nextIndex);
      setVisibleUpTo(steps.length - 1);
      onComplete?.();
      return;
    }

    setCurrentIndex(nextIndex);
    const delay = steps[nextIndex]?.delay ?? 0;

    if (delay > 0) {
      setIsDelaying(true);
      clearDelay();
      delayRef.current = setTimeout(() => {
        setVisibleUpTo(nextIndex);
        setIsDelaying(false);
      }, delay);
      return;
    }

    setVisibleUpTo(nextIndex);
  }, [currentIndex, steps, onComplete, clearDelay]);

  useEffect(() => {
    clearDelay();
    setCurrentIndex(0);
    setVisibleUpTo(-1);
    setIsDelaying(false);

    if (!steps.length) return;

    const firstDelay = steps[0]?.delay ?? 250;
    delayRef.current = setTimeout(() => setVisibleUpTo(0), firstDelay);

    return () => clearDelay();
  }, [steps, sessionKey, clearDelay]);

  useEffect(() => {
    if (!activeStep?.autoAdvance) return;
    if (isFinished || isDelaying || visibleUpTo < currentIndex) return;

    const timeout = setTimeout(() => next(), activeStep.autoAdvance);
    return () => clearTimeout(timeout);
  }, [activeStep, isFinished, isDelaying, visibleUpTo, currentIndex, next]);

  useEffect(() => {
    if (!scrollRef.current) return;
    scrollRef.current.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [visibleUpTo, currentIndex, isFinished, isDelaying]);

  useEffect(() => {
    return () => clearDelay();
  }, [clearDelay]);

  const handleRestart = () => {
    setSessionKey((prev) => prev + 1);
  };

  const renderStep = (step: Step, index: number) => {
    const isActive = index === currentIndex && index <= visibleUpTo && !isFinished;

    switch (step.type) {
      case "slack-message":
        return (
          <SlackMessage
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      case "reply":
        return (
          <ReplyComposer
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      case "code-editor":
        return (
          <CodeEditor
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      case "status":
        return (
          <StatusPanel
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      default:
        return null;
    }
  };

  const containerHeight =
    mode === "hero" ? "min-h-[280px] max-h-[560px]" : "min-h-[300px] max-h-[70vh]";

  return (
    <div
      className={`rounded-xl border border-iii-medium/20 bg-iii-black/80 backdrop-blur-sm flex flex-col overflow-hidden ${containerHeight} ${className}`}
    >
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-iii-medium/10 bg-iii-dark/40 shrink-0">
        <div className="flex items-center gap-2">
          <div className="flex gap-1">
            <span className="w-2 h-2 rounded-full bg-iii-alert/60" />
            <span className="w-2 h-2 rounded-full bg-iii-warn/60" />
            <span className="w-2 h-2 rounded-full bg-iii-success/60" />
          </div>
          <span className="text-xs text-iii-medium font-mono ml-2">iii interactive flow</span>
        </div>
        <button
          onClick={handleRestart}
          className="text-[10px] text-iii-medium/60 hover:text-iii-accent transition-colors cursor-pointer"
        >
          restart
        </button>
      </div>

      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto px-4 py-4 space-y-4 scrollbar-brand-dark"
      >
        {steps.map((step, index) => {
          if (index > visibleUpTo) return null;
          return <div key={`${sessionKey}-${step.id}`}>{renderStep(step, index)}</div>;
        })}

        {isDelaying && (
          <div className="flex gap-1.5 items-center px-2 py-2">
            <span
              className="w-1.5 h-1.5 rounded-full bg-iii-medium/40 animate-bounce"
              style={{ animationDelay: "0ms" }}
            />
            <span
              className="w-1.5 h-1.5 rounded-full bg-iii-medium/40 animate-bounce"
              style={{ animationDelay: "150ms" }}
            />
            <span
              className="w-1.5 h-1.5 rounded-full bg-iii-medium/40 animate-bounce"
              style={{ animationDelay: "300ms" }}
            />
          </div>
        )}

        {isFinished && (
          <div
            className="text-center py-6 animate-[fadeSlideIn_0.4s_ease-out_forwards] opacity-0"
            style={{ animationFillMode: "forwards" }}
          >
            <p className="text-sm text-iii-accent font-bold mb-1">[Flow complete placeholder]</p>
            <p className="text-xs text-iii-medium">
              [Use this area for CTA text in homepage or onboarding]
            </p>
            <button
              onClick={handleRestart}
              className="mt-4 px-5 py-2 text-xs font-bold rounded bg-iii-accent text-iii-black hover:brightness-110 transition-all duration-200 cursor-pointer"
            >
              Run again
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
