import { motion } from 'motion/react';
import { useEffect, useState } from 'react';
import { AnimationShape, ShapeType } from './AnimationShape';

const CENTER_COLOR = 'var(--color-accent)';

const SHAPES: {
  id: number;
  color: string;
  type: ShapeType;
  dx: number;
  dy: number;
  cx: number;
  cy: number;
}[] = [
  { id: 0, color: '#42e7e7', type: 'circle', dx: -40, dy: 0, cx: -23, cy: 0 }, // Blue
  { id: 1, color: '#f3943d', type: 'square', dx: 0, dy: -40, cx: 0, cy: -23 }, // Orange
  { id: 2, color: '#ef2e61', type: 'diamond', dx: 40, dy: 0, cx: 23, cy: 0 }, // Red
];

function getBackground(colors: string[]) {
  if (colors.length === 0) return CENTER_COLOR;
  if (colors.length === 1) return colors[0];
  if (colors.length === 2) {
    return `linear-gradient(90deg, ${colors[0]} 50%, ${colors[1]} 50%)`;
  }
  if (colors.length === 3) {
    return `linear-gradient(90deg, ${colors[0]} 33.33%, ${colors[1]} 33.33% 66.66%, ${colors[2]} 66.66%)`;
  }
  return CENTER_COLOR;
}

export function LiveSystemAnimation() {
  const [step, setStep] = useState(0);
  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    const interval = setInterval(() => {
      setStep((s) => (s + 1) % 6);
    }, 1500);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    const mediaQuery = window.matchMedia('(max-width: 639px)');
    const handleMediaChange = (event: MediaQueryListEvent) => {
      setIsMobile(event.matches);
    };

    setIsMobile(mediaQuery.matches);

    if (mediaQuery.addEventListener) {
      mediaQuery.addEventListener('change', handleMediaChange);
      return () => mediaQuery.removeEventListener('change', handleMediaChange);
    }

    mediaQuery.addListener(handleMediaChange);
    return () => mediaQuery.removeListener(handleMediaChange);
  }, []);

  // Determine connected shapes based on step
  let connected: number[] = [];

  if (step === 1) connected = [0];
  else if (step === 2) connected = [0, 1];
  else if (step === 3) connected = [0, 1, 2];
  else if (step === 4) connected = [0, 1];
  else if (step === 5) connected = [0];
  else connected = [];

  const connectedColors = connected.map((id) => SHAPES[id].color);
  const activeBackground = getBackground(connectedColors);
  const connectedOffset = isMobile ? 20 : 23;

  return (
    <div className="relative w-24 h-24 flex items-center justify-center">
      {/* Central Box */}
      <motion.div
        className="absolute w-7 h-7 sm:w-8 sm:h-8 border border-black z-10 rounded-sm"
        style={{ backgroundColor: CENTER_COLOR }}
        animate={{ background: activeBackground }}
        transition={{ duration: 0.3 }}
      />

      {/* Shapes */}
      {SHAPES.map((shape) => {
        const isConnected = connected.includes(shape.id);
        const background = isConnected ? activeBackground : shape.color;
        const x = isConnected
          ? shape.cx === 0
            ? 0
            : shape.cx > 0
              ? connectedOffset
              : -connectedOffset
          : shape.dx;
        const y = isConnected
          ? shape.cy === 0
            ? 0
            : shape.cy > 0
              ? connectedOffset
              : -connectedOffset
          : shape.dy;

        return (
          <AnimationShape
            key={shape.id}
            type={shape.type}
            color={background}
            className="w-4 h-4 sm:w-[22px] sm:h-[22px] z-20"
            animate={{ x, y }}
            transition={{ duration: 0.5, type: 'spring', bounce: 0.4 }}
          />
        );
      })}
    </div>
  );
}
