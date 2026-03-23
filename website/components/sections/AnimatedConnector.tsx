import React from 'react';

interface AnimatedConnectorProps {
  isDarkMode: boolean;
  orientation?: 'vertical' | 'horizontal';
  length?: number;
  begin?: string;
  duration?: string;
  highlightLine?: boolean;
  className?: string;
}

export function AnimatedConnector({
  isDarkMode,
  orientation = 'vertical',
  length = 28,
  begin = '0s',
  duration = '1.8s',
  highlightLine = false,
  className,
}: AnimatedConnectorProps) {
  const stroke = highlightLine
    ? isDarkMode
      ? '#f3f724'
      : '#2f7fff'
    : isDarkMode
      ? 'rgba(255,255,255,0.15)'
      : 'rgba(0,0,0,0.1)';
  const dotColor = isDarkMode ? '#ffffff' : '#000000';
  const baseBeginSeconds = Number.parseFloat(begin) || 0;
  const dotOffsetsForward = [0, 0.42, 0.84];
  const dotOffsetsReverse = [0.21, 0.63, 1.05];

  if (orientation === 'horizontal') {
    return (
      <svg
        width={length}
        height="8"
        viewBox={`0 0 ${length} 8`}
        preserveAspectRatio="none"
        className={className}
      >
        <line
          x1="0"
          y1="4"
          x2={length}
          y2="4"
          stroke={stroke}
          strokeWidth="1.5"
          strokeLinecap="round"
        />
        {dotOffsetsForward.map((offset, index) => (
          <circle
            key={`h-forward-${index}`}
            r={index === 0 ? 2 : index === 1 ? 1.8 : 1.6}
            fill={dotColor}
            opacity={index === 0 ? 0.72 : index === 1 ? 0.58 : 0.46}
          >
            <animateMotion
              dur={duration}
              begin={`${(baseBeginSeconds + offset).toFixed(2)}s`}
              repeatCount="indefinite"
              path={`M 0,4 L ${length},4`}
            />
          </circle>
        ))}
        {dotOffsetsReverse.map((offset, index) => (
          <circle
            key={`h-reverse-${index}`}
            r={index === 0 ? 1.9 : index === 1 ? 1.7 : 1.5}
            fill={dotColor}
            opacity={index === 0 ? 0.64 : index === 1 ? 0.52 : 0.4}
          >
            <animateMotion
              dur={duration}
              begin={`${(baseBeginSeconds + offset).toFixed(2)}s`}
              repeatCount="indefinite"
              path={`M ${length},4 L 0,4`}
            />
          </circle>
        ))}
      </svg>
    );
  }

  return (
    <svg width="2" height={length} className={className}>
      <line
        x1="1"
        y1="0"
        x2="1"
        y2={length}
        stroke={stroke}
        strokeWidth="1.5"
        strokeLinecap="round"
      />
      {dotOffsetsForward.map((offset, index) => (
        <circle
          key={`v-forward-${index}`}
          r={index === 0 ? 2 : index === 1 ? 1.8 : 1.6}
          fill={dotColor}
          opacity={index === 0 ? 0.72 : index === 1 ? 0.58 : 0.46}
        >
          <animateMotion
            dur={duration}
            begin={`${(baseBeginSeconds + offset).toFixed(2)}s`}
            repeatCount="indefinite"
            path={`M 1,0 L 1,${length}`}
          />
        </circle>
      ))}
      {dotOffsetsReverse.map((offset, index) => (
        <circle
          key={`v-reverse-${index}`}
          r={index === 0 ? 1.9 : index === 1 ? 1.7 : 1.5}
          fill={dotColor}
          opacity={index === 0 ? 0.64 : index === 1 ? 0.52 : 0.4}
        >
          <animateMotion
            dur={duration}
            begin={`${(baseBeginSeconds + offset).toFixed(2)}s`}
            repeatCount="indefinite"
            path={`M 1,${length} L 1,0`}
          />
        </circle>
      ))}
    </svg>
  );
}
