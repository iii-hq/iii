import { motion } from 'motion/react';
import { useEffect, useState } from 'react';
import { AnimationShape, ShapeType } from './AnimationShape';

const SHAPES: { id: number; color: string; type: ShapeType; dx: number; dy: number; cx: number; cy: number }[] = [
  { id: 0, color: '#3b82f6', type: 'circle', dx: -35, dy: 0, cx: -20, cy: 0 },
  { id: 1, color: '#ef4444', type: 'square', dx: 0, dy: -35, cx: 0, cy: -20 },
  { id: 2, color: '#10b981', type: 'diamond', dx: 35, dy: 0, cx: 20, cy: 0 },
];

function getBackground(colors: string[]) {
  if (colors.length === 0) return '#ffffff';
  if (colors.length === 1) return colors[0];
  if (colors.length === 2) {
    return `linear-gradient(135deg, ${colors[0]} 50%, ${colors[1]} 50%)`;
  }
  if (colors.length === 3) {
    return `linear-gradient(135deg, ${colors[0]} 33.33%, ${colors[1]} 33.33% 66.66%, ${colors[2]} 66.66%)`;
  }
  return '#ffffff';
}

export function LiveSystemAnimation() {
  const [step, setStep] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setStep((s) => (s + 1) % 6);
    }, 1500);
    return () => clearInterval(interval);
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

  return (
    <div className="relative w-24 h-24 flex items-center justify-center">
      {/* Central Box */}
      <motion.div
        className="absolute w-8 h-8 bg-white border border-black z-10 rounded-sm"
        animate={{ background: activeBackground }}
        transition={{ duration: 0.3 }}
      />

      {/* Shapes */}
      {SHAPES.map((shape) => {
        const isConnected = connected.includes(shape.id);
        const background = isConnected ? activeBackground : shape.color;
        const x = isConnected ? shape.cx : shape.dx;
        const y = isConnected ? shape.cy : shape.dy;

        return (
          <AnimationShape
            key={shape.id}
            type={shape.type}
            color={background}
            className="w-4 h-4 z-20"
            animate={{ x, y }}
            transition={{ duration: 0.5, type: 'spring', bounce: 0.4 }}
          />
        );
      })}
    </div>
  );
}
