import { useCallback, useEffect, useRef, useState } from 'react';
import type { DemoSequencerProps, Step } from './types';
import { ChatMessage } from './ChatMessage';
import { ReplyComposer } from './ReplyComposer';
import { CodeEditor } from './CodeEditor';
import { StatusPanel } from './StatusPanel';
import { InstallShButton } from '../InstallShButton';

function SlackLogo() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="w-3.5 h-3.5 shrink-0"
      aria-hidden="true"
    >
      <path
        fill="#E01E5A"
        d="M5.042 15.165a2.528 2.528 0 1 1-2.527-2.527h2.527v2.527zm1.273 0a2.528 2.528 0 1 1 5.055 0v6.319a2.528 2.528 0 0 1-5.055 0v-6.319z"
      />
      <path
        fill="#36C5F0"
        d="M8.842 5.042a2.528 2.528 0 1 1 2.528-2.527v2.527H8.842zm0 1.273a2.528 2.528 0 1 1 0 5.055H2.527a2.528 2.528 0 0 1 0-5.055h6.315z"
      />
      <path
        fill="#2EB67D"
        d="M18.958 8.842a2.528 2.528 0 1 1 2.527 2.528h-2.527V8.842zm-1.273 0a2.528 2.528 0 1 1-5.055 0V2.527a2.528 2.528 0 0 1 5.055 0v6.315z"
      />
      <path
        fill="#ECB22E"
        d="M15.158 18.965a2.528 2.528 0 1 1-2.528 2.527v-2.527h2.528zm0-1.273a2.528 2.528 0 1 1 0-5.055h6.315a2.528 2.528 0 0 1 0 5.055h-6.315z"
      />
    </svg>
  );
}

export function DemoSequencer({
  steps,
  mode = 'hero',
  onComplete,
  className = '',
}: DemoSequencerProps) {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [visibleUpTo, setVisibleUpTo] = useState(-1);
  const [isDelaying, setIsDelaying] = useState(false);
  const [sessionKey, setSessionKey] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const delayRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const isFinished = currentIndex >= steps.length;

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
    if (!scrollRef.current) return;
    const scroll = scrollRef.current;
    const pin = () => {
      requestAnimationFrame(() => {
        scroll.scrollTop = scroll.scrollHeight;
      });
    };
    const observer = new MutationObserver(pin);
    observer.observe(scroll, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    return () => observer.disconnect();
  }, [sessionKey]);

  useEffect(() => {
    return () => clearDelay();
  }, [clearDelay]);

  const handleRestart = () => {
    setSessionKey((prev) => prev + 1);
  };

  const renderStep = (step: Step, index: number) => {
    const isActive =
      index === currentIndex && index <= visibleUpTo && !isFinished;

    switch (step.type) {
      case 'slack-message':
        return (
          <ChatMessage
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      case 'reply':
        return (
          <ReplyComposer
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      case 'code-editor':
        return (
          <CodeEditor
            key={`${sessionKey}-${step.id}`}
            step={step}
            isActive={isActive}
            onNext={next}
          />
        );
      case 'status':
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

  const containerHeight = mode === 'hero' ? 'h-[560px]' : 'h-[70vh]';

  return (
    <div
      className={`rounded-xl border border-iii-medium/20 bg-iii-black/80 backdrop-blur-sm flex flex-col overflow-hidden ${containerHeight} ${className}`}
    >
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-iii-medium/10 bg-iii-dark/40 shrink-0">
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5" aria-hidden="true">
            <span className="h-2.5 w-2.5 rounded-full bg-iii-alert shadow-[inset_0_1px_0_rgba(255,255,255,0.35)]" />
            <span className="h-2.5 w-2.5 rounded-full bg-iii-warn shadow-[inset_0_1px_0_rgba(255,255,255,0.35)]" />
            <span className="h-2.5 w-2.5 rounded-full bg-iii-success shadow-[inset_0_1px_0_rgba(255,255,255,0.35)]" />
          </div>
          <span className="flex items-center gap-1.5 ml-2">
            <SlackLogo />
            <span className="text-xs text-iii-medium font-mono">Slack</span>
          </span>
        </div>
        <button
          onClick={handleRestart}
          className="text-[10px] text-iii-medium/60 hover:text-iii-accent transition-colors cursor-pointer"
        >
          start over
        </button>
      </div>

      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto px-4 py-4 space-y-4 scrollbar-brand-dark"
      >
        {steps.map((step, index) => {
          if (index > visibleUpTo) return null;
          return (
            <div key={`${sessionKey}-${step.id}`}>
              {renderStep(step, index)}
            </div>
          );
        })}

        {isDelaying && (
          <div className="flex gap-1.5 items-center px-2 py-2">
            <span
              className="w-1.5 h-1.5 rounded-full bg-iii-medium/40 animate-bounce"
              style={{ animationDelay: '0ms' }}
            />
            <span
              className="w-1.5 h-1.5 rounded-full bg-iii-medium/40 animate-bounce"
              style={{ animationDelay: '150ms' }}
            />
            <span
              className="w-1.5 h-1.5 rounded-full bg-iii-medium/40 animate-bounce"
              style={{ animationDelay: '300ms' }}
            />
          </div>
        )}

        {isFinished && (
          <div
            className="text-center py-6 animate-[fadeSlideIn_0.4s_ease-out_forwards] opacity-0"
            style={{ animationFillMode: 'forwards' }}
          >
            <p className="text-sm text-iii-accent font-bold mb-3">
              Try it for yourself
            </p>
            <div className="flex justify-center">
              <InstallShButton className="max-w-[280px]" />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
