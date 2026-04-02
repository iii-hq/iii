import React from "react";

interface ModeToggleProps {
  isHumanMode: boolean;
  onToggle: () => void;
  isDarkMode: boolean;
}

export const ModeToggle: React.FC<ModeToggleProps> = ({
  isHumanMode,
  onToggle,
  isDarkMode,
}) => {
  const bgColor = isDarkMode ? "bg-iii-dark/50" : "bg-iii-medium/10";
  const borderColor = isDarkMode
    ? "border-iii-medium/20"
    : "border-iii-medium/20";

  return (
    <div
      className={`flex items-center rounded-full p-0.5 border-2 ${bgColor} ${borderColor}`}
    >
      <button
        onClick={() => !isHumanMode && onToggle()}
        className={`px-2.5 py-1 md:px-3 md:py-1.5 rounded-full text-[10px] md:text-xs font-medium tracking-wide transition-all duration-200 uppercase ${
          isHumanMode
            ? isDarkMode
              ? "bg-iii-black text-iii-light"
              : "bg-white text-iii-black shadow-sm"
            : isDarkMode
            ? "text-iii-medium hover:text-iii-light"
            : "text-iii-medium hover:text-iii-black"
        }`}
        aria-label="Human mode"
      >
        Human
      </button>
      <button
        onClick={() => isHumanMode && onToggle()}
        className={`px-2.5 py-1 md:px-3 md:py-1.5 rounded-full text-[10px] md:text-xs font-medium tracking-wide transition-all duration-200 uppercase ${
          !isHumanMode
            ? "bg-black text-white"
            : isDarkMode
            ? "text-iii-medium hover:text-iii-light"
            : "text-iii-medium hover:text-iii-black"
        }`}
        aria-label="Machine mode"
      >
        Machine
      </button>
    </div>
  );
};
