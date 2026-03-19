import { useEffect, useMemo, useRef, useState } from "react";
import type { ReplyStep } from "./types";

interface ReplyComposerProps {
  step: ReplyStep;
  isActive: boolean;
  onNext: () => void;
}

export function ReplyComposer({ step, isActive, onNext }: ReplyComposerProps) {
  const [displayedWords, setDisplayedWords] = useState<string[]>([]);
  const [isTyping, setIsTyping] = useState(false);
  const [isDone, setIsDone] = useState(false);
  const [sent, setSent] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const words = useMemo(() => step.content.split(" "), [step.content]);
  const speed = step.typingSpeed ?? 40;
  const visibleContent = isDone ? step.content : displayedWords.join(" ");

  useEffect(() => {
    if (!isActive || isDone) return;

    setIsTyping(true);
    let wordIndex = 0;

    intervalRef.current = setInterval(() => {
      if (wordIndex < words.length) {
        setDisplayedWords((prev) => [...prev, words[wordIndex]]);
        wordIndex += 1;
        return;
      }

      if (intervalRef.current) clearInterval(intervalRef.current);
      setIsTyping(false);
      setIsDone(true);
    }, speed);

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [isActive, isDone, words, speed]);

  const handleSend = () => {
    if (sent) return;
    setSent(true);
    onNext();
  };

  useEffect(() => {
    if (!isActive || !isDone || sent || !step.autoAdvance) return;
    const timer = setTimeout(() => {
      setSent(true);
      onNext();
    }, step.autoAdvance);
    return () => clearTimeout(timer);
  }, [isActive, isDone, sent, step.autoAdvance, onNext]);

  if (!isActive && sent) {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] px-3 py-2 rounded-lg bg-iii-accent/10 border border-iii-accent/20">
          <p className="text-sm text-iii-light/90">{step.content}</p>
        </div>
      </div>
    );
  }

  return (
    <div
      className={`flex justify-end ${
        isActive
          ? "animate-[fadeSlideIn_0.3s_ease-out_forwards] opacity-0"
          : "opacity-70"
      }`}
      style={isActive ? { animationFillMode: "forwards" } : undefined}
    >
      <div className="max-w-[85%] w-full">
        <div className="rounded-lg border border-iii-medium/20 bg-iii-dark/80">
          <div className="px-3 py-2.5 min-h-[40px]">
            <p className="text-sm text-iii-light/90 leading-relaxed">
              {visibleContent}
              {isTyping && (
                <span className="inline-block w-[2px] h-[14px] bg-iii-accent ml-0.5 animate-pulse align-middle" />
              )}
            </p>
          </div>

          <div className="flex justify-end px-3 py-2 border-t border-iii-medium/10">
            {isDone && !sent && !step.autoAdvance ? (
              <button
                onClick={handleSend}
                className="px-4 py-1.5 text-xs font-bold rounded bg-iii-accent text-iii-black hover:brightness-110 transition-all duration-200 cursor-pointer animate-[fadeSlideIn_0.2s_ease-out_forwards]"
              >
                {step.sendLabel ?? "Send"}
              </button>
            ) : (
              <div className="px-4 py-1.5 text-xs text-iii-medium/40">
                {isTyping ? "typing..." : ""}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
