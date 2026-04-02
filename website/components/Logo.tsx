import React from "react";

interface LogoProps {
  className?: string;
  highlightIndex?: number;
  highlightCount?: number;
  accentColor?: string;
}

export const Logo: React.FC<LogoProps> = ({
  className = "h-8",
  highlightIndex,
  highlightCount,
  accentColor = "fill-iii-accent",
}) => {
  const getBarClass = (index: number) => {
    if (highlightCount !== undefined && index < highlightCount) {
      return `${accentColor} transition-all duration-150`;
    }
    if (highlightIndex !== undefined && highlightIndex === index) {
      return `${accentColor} transition-all duration-150`;
    }
    return "fill-current transition-all duration-150";
  };

  return (
    <svg
      viewBox="0 0 1075.74 1075.74"
      className={`${className} transition-transform duration-150`}
      xmlns="http://www.w3.org/2000/svg"
    >
      <rect
        className={getBarClass(0)}
        x="0"
        y="0.05"
        width="268.94"
        height="268.94"
      />
      <rect
        className={getBarClass(0)}
        x="0"
        y="403.45"
        width="268.94"
        height="672.24"
      />

      <rect
        className={getBarClass(1)}
        x="403.4"
        y="0.05"
        width="268.94"
        height="268.94"
      />
      <rect
        className={getBarClass(1)}
        x="403.4"
        y="403.45"
        width="268.94"
        height="672.24"
      />

      <rect
        className={getBarClass(2)}
        x="806.81"
        y="0.05"
        width="268.94"
        height="268.94"
      />
      <rect
        className={getBarClass(2)}
        x="806.81"
        y="403.45"
        width="268.94"
        height="672.24"
      />
    </svg>
  );
};
