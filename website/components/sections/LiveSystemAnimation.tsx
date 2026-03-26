import { motion } from 'motion/react';
import { useEffect, useState } from 'react';

const SHAPES = [
  { id: 0, color: '#3b82f6', type: 'circle', dx: -35, dy: 0, cx: -20, cy: 0 },
  { id: 1, color: '#ef4444', type: 'square', dx: 0, dy: -35, cx: 0, cy: -20 },
  { id: 2, color: '#10b981', type: 'diamond', dx: 35, dy: 0, cx: 20, cy: 0 },
];

export function LiveSystemAnimation() {
  const [step, setStep] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setStep((s) => (s + 1) % 6);
    }, 1500);
    return () => clearInterval(interval);
  }, []);

  // Determine connected shapes and active color based on step
  let connected: number[] = [];
  let activeColor = '#ffffff';

  if (step === 1) {
    connected = [0];
    activeColor = SHAPES[0].color;
  } else if (step === 2) {
    connected = [0, 1];
    activeColor = SHAPES[1].color;
  } else if (step === 3) {
    connected = [0, 1, 2];
    activeColor = SHAPES[2].color;
  } else if (step === 4) {
    connected = [0, 1];
    activeColor = '#ffffff';
  } else if (step === 5) {
    connected = [0];
    activeColor = '#ffffff';
  }

  return (
    <div className="relative w-24 h-24 flex items-center justify-center">
      {/* Central Box */}
      <motion.div
        className="absolute w-8 h-8 bg-white border border-neutral-200 dark:border-neutral-700 z-10 rounded-sm"
        animate={{ backgroundColor: activeColor }}
        transition={{ duration: 0.3 }}
      />

      {/* Shapes */}
      {SHAPES.map((shape) => {
        const isConnected = connected.includes(shape.id);
        const color = isConnected ? activeColor : shape.color;
        const x = isConnected ? shape.cx : shape.dx;
        const y = isConnected ? shape.cy : shape.dy;

        let shapeClasses = "absolute w-4 h-4 z-20 border border-neutral-200 dark:border-neutral-700 ";
        if (shape.type === 'circle') shapeClasses += "rounded-full";
        else if (shape.type === 'diamond') shapeClasses += "rotate-45 rounded-sm";
        else if (shape.type === 'square') shapeClasses += "rounded-sm";

        return (
          <motion.div
            key={shape.id}
            className={shapeClasses}
            animate={{
              x,
              y,
              backgroundColor: color,
              rotate: shape.type === 'diamond' ? 45 : 0,
              borderRadius: shape.type === 'circle' ? '50%' : '2px'
            }}
            transition={{ duration: 0.5, type: 'spring', bounce: 0.4 }}
          />
        );
      })}
    </div>
  );
}
