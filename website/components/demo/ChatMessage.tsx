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
          : "opacity-80"
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
          <button
            onClick={handleClick}
            className="w-full sm:w-auto px-4 py-1.5 text-xs font-bold rounded bg-iii-accent text-iii-black hover:brightness-110 transition-all duration-200 animate-[socketPulseInline_2s_ease-in-out_infinite] cursor-pointer"
          >
            {step.action.label}
          </button>
        </div>
      )}
    </div>
  );
}
