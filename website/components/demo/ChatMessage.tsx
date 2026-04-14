import { useEffect, useState } from "react";
import type { SlackMessageStep } from "./types";

interface ChatMessageProps {
  step: SlackMessageStep;
  isActive: boolean;
  onNext: () => void;
}

function AvatarInitials({ name }: { name: string }) {
  const initials = name
    .split(" ")
    .map((word) => word[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();

  return (
    <div className="w-8 h-8 rounded-md bg-iii-medium/30 flex items-center justify-center text-xs font-bold text-iii-light shrink-0">
      {initials}
    </div>
  );
}

const STREAM_CHARS = '01│─┌┐└┘├┤iii';

function DataStreamButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="relative overflow-hidden w-full sm:w-auto px-10 py-4 rounded font-bold text-base cursor-pointer group bg-iii-dark border border-iii-accent/50 text-iii-accent"
    >
      <div
        aria-hidden="true"
        className="absolute inset-0 pointer-events-none overflow-hidden text-iii-accent"
      >
        {Array.from({ length: 5 }).map((_, row) => (
          <div
            key={row}
            className="absolute whitespace-nowrap text-[9px] font-mono tracking-[2px]"
            style={{
              top: `${5 + row * 22}%`,
              opacity: 0.3,
              animation: `streamFlow ${4 + row * 0.8}s linear infinite`,
              animationDelay: `${row * -0.4}s`,
            }}
          >
            {Array.from({ length: 200 }).map((_, i) => (
              <span key={i}>{STREAM_CHARS[Math.floor(Math.random() * STREAM_CHARS.length)]}</span>
            ))}
          </div>
        ))}
      </div>
      <span
        className="relative z-10 text-iii-light group-hover:text-iii-accent transition-colors"
        style={{ WebkitTextStroke: '4px var(--color-dark)', paintOrder: 'stroke fill' }}
      >{label}</span>
      <style>{`
        @keyframes streamFlow {
          0% { transform: translateX(-50%); }
          100% { transform: translateX(0%); }
        }
      `}</style>
    </button>
  );
}

export function ChatMessage({ step, isActive, onNext }: ChatMessageProps) {
  const [clicked, setClicked] = useState(false);

  const handleClick = () => {
    if (!isActive || clicked) return;
    setClicked(true);
    onNext();
  };

  useEffect(() => {
    if (!isActive || clicked || !step.autoAdvance) return;
    const timer = setTimeout(() => {
      setClicked(true);
      onNext();
    }, step.autoAdvance);
    return () => clearTimeout(timer);
  }, [isActive, clicked, step.autoAdvance, onNext]);

  return (
    <div
      className={`flex flex-wrap xl:flex-nowrap gap-3 items-start ${
        isActive
          ? "animate-[fadeSlideIn_0.3s_ease-out_forwards] opacity-0"
          : ""
      }`}
      style={isActive ? { animationFillMode: "forwards" } : undefined}
    >
      {step.sender.avatar ? (
        <img
          src={step.sender.avatar}
          alt={step.sender.name}
          className="w-8 h-8 rounded-md shrink-0"
        />
      ) : (
        <AvatarInitials name={step.sender.name} />
      )}

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-sm font-bold text-iii-light">{step.sender.name}</span>
          {step.sender.role && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-iii-medium/20 text-iii-medium uppercase tracking-wider">
              {step.sender.role}
            </span>
          )}
        </div>

        <div className="text-sm text-iii-light/90 leading-relaxed">{step.content}</div>

        {isActive && !step.autoAdvance && !step.action && !clicked && (
          <button
            onClick={handleClick}
            className="mt-2 text-[10px] text-iii-medium hover:text-iii-accent transition-colors cursor-pointer"
          >
            Continue
          </button>
        )}
      </div>

      {isActive && !step.autoAdvance && step.action && !clicked && (
        <div className="basis-full pl-11 flex justify-end xl:basis-auto xl:pl-0 xl:self-end xl:shrink-0">
          <DataStreamButton label={step.action.label} onClick={handleClick} />
        </div>
      )}
    </div>
  );
}
