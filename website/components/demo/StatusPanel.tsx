import { useEffect, useState } from "react";
import type { StatusStep } from "./types";

interface StatusPanelProps {
  step: StatusStep;
  isActive: boolean;
  onNext: () => void;
}

const variantClasses: Record<StatusStep["variant"], string> = {
  success: "text-iii-success border-iii-success/30 bg-iii-success/5",
  info: "text-iii-info border-iii-info/30 bg-iii-info/5",
  warn: "text-iii-warn border-iii-warn/30 bg-iii-warn/5",
  alert: "text-iii-alert border-iii-alert/30 bg-iii-alert/5",
  accent: "text-iii-accent border-iii-accent/30 bg-iii-accent/5",
};

const iconMap: Record<string, string> = {
  check: "✓",
  connect: "⬡",
  deploy: "▲",
  observe: "◉",
  bolt: "⚡",
};

export function StatusPanel({ step, isActive, onNext }: StatusPanelProps) {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const timeout = setTimeout(() => setVisible(true), 60);
    return () => clearTimeout(timeout);
  }, []);

  useEffect(() => {
    if (!isActive || !step.autoAdvance) return;
    const timer = setTimeout(onNext, step.autoAdvance);
    return () => clearTimeout(timer);
  }, [isActive, step.autoAdvance, onNext]);

  const icon = step.icon ? iconMap[step.icon] ?? "•" : "•";
  const classes = variantClasses[step.variant] ?? variantClasses.info;

  return (
    <div
      className={`flex items-center gap-3 px-4 py-2.5 rounded-lg border transition-all duration-300 ${classes} ${
        visible ? "opacity-100 translate-x-0" : "opacity-0 translate-x-4"
      } ${isActive ? "" : ""}`}
    >
      <span className="text-base shrink-0">{icon}</span>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-bold">{step.headline}</p>
        {step.detail && <p className="text-xs opacity-70 mt-0.5">{step.detail}</p>}
      </div>
      {isActive && !step.autoAdvance && (
        <button
          onClick={onNext}
          className="text-[10px] opacity-50 hover:opacity-100 transition-opacity cursor-pointer shrink-0"
        >
          continue
        </button>
      )}
    </div>
  );
}
