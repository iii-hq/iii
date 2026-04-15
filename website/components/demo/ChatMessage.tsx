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

function BadgeButton({ label, count, onClick }: { label: string; count?: number; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="relative px-5 py-2.5 rounded font-bold text-sm cursor-pointer bg-iii-accent text-black hover:brightness-110 transition-all animate-[btnWiggle_1s_ease-in-out_3s_infinite] hover:animate-none"
    >
      {label}
      {count != null && (
        <span className="absolute -top-1.5 -right-1.5 flex items-center justify-center min-w-[20px] h-5 px-1 rounded-full bg-red-500 text-white text-[11px] font-bold shadow-sm ring-2 ring-iii-dark opacity-0 animate-[badgePopIn_0.4s_ease-out_1s_forwards]">
          <span className="absolute inset-0 rounded-full bg-red-500 animate-[badgePing_2s_ease-out_1.5s_infinite]" />
          <span className="relative">{count}</span>
        </span>
      )}
      <style>{`
        @keyframes badgePopIn {
          0% { transform: scale(0); opacity: 0; }
          60% { transform: scale(1.25); opacity: 1; }
          80% { transform: scale(0.9); }
          100% { transform: scale(1); opacity: 1; }
        }
        @keyframes badgePing {
          0% { transform: scale(1); opacity: 0.8; }
          100% { transform: scale(2.2); opacity: 0; }
        }
        @keyframes btnWiggle {
          0%, 100% { transform: rotate(0deg); }
          20% { transform: rotate(-3deg); }
          40% { transform: rotate(3deg); }
          60% { transform: rotate(-2deg); }
          80% { transform: rotate(2deg); }
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
          <BadgeButton label={step.action.label} count={step.action.count} onClick={handleClick} />
        </div>
      )}
    </div>
  );
}
