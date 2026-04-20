import { useEffect, useRef, useState } from "react";
import type { TerminalCommandStep } from "./types";

interface TerminalCommandProps {
  step: TerminalCommandStep;
  isActive: boolean;
  onNext: () => void;
}

export function TerminalCommand({ step, isActive, onNext }: TerminalCommandProps) {
  const [typedChars, setTypedChars] = useState(0);
  const [showOutput, setShowOutput] = useState(false);
  const speed = step.typingSpeed ?? 40;
  const doneRef = useRef(false);

  useEffect(() => {
    if (!isActive || doneRef.current) return;

    if (typedChars < step.command.length) {
      const timer = setTimeout(() => setTypedChars((c) => c + 1), speed);
      return () => clearTimeout(timer);
    }

    if (!showOutput) {
      const timer = setTimeout(() => setShowOutput(true), 300);
      return () => clearTimeout(timer);
    }

    if (!doneRef.current) {
      doneRef.current = true;
      const timer = setTimeout(onNext, step.autoAdvance ?? 800);
      return () => clearTimeout(timer);
    }
  }, [isActive, typedChars, showOutput, step.command.length, speed, step.autoAdvance, onNext]);

  const typing = typedChars < step.command.length;

  return (
    <div className="rounded-lg border border-iii-medium/20 bg-iii-dark overflow-hidden font-mono text-xs">
      <div className="flex items-center gap-1.5 px-3 py-1.5 bg-iii-black/50 border-b border-iii-medium/10">
        <span className="text-iii-medium/60 text-[10px]">terminal</span>
      </div>
      <div className="px-3 py-2.5 space-y-1">
        <div className="flex items-center gap-1.5">
          <span className="text-iii-accent shrink-0">&gt;</span>
          <span className="text-iii-light">
            {step.command.slice(0, typedChars)}
          </span>
          {typing && (
            <span className="inline-block w-[6px] h-[14px] bg-iii-light animate-pulse" />
          )}
        </div>
        {showOutput && (
          <div className="text-iii-success animate-[fadeSlideIn_0.2s_ease-out_forwards]">
            {step.output}
          </div>
        )}
      </div>
    </div>
  );
}
