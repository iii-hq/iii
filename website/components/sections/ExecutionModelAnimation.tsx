import { motion } from 'motion/react';
import { useEffect, useState } from 'react';
import { AnimationShape, ShapeType } from './AnimationShape';

const WORKERS: { id: number; type: ShapeType; color: string; x: number; y: number }[] = [
  { id: 0, type: 'circle', color: '#3b82f6', x: 0, y: -28 },
  { id: 1, type: 'square', color: '#ef4444', x: -24, y: 14 },
  { id: 2, type: 'diamond', color: '#10b981', x: 24, y: 14 },
];

export function ExecutionModelAnimation() {
  const [step, setStep] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setStep((s) => (s + 1) % 8);
    }, 800);
    return () => clearInterval(interval);
  }, []);

  let dotState = { x: 0, y: 0, opacity: 0, color: '#000000' };
  
  switch (step) {
    case 0: dotState = { x: 0, y: 0, opacity: 0, color: WORKERS[0].color }; break;
    case 1: dotState = { x: WORKERS[0].x, y: WORKERS[0].y, opacity: 1, color: WORKERS[0].color }; break; // Success W1
    case 2: dotState = { x: 0, y: 0, opacity: 0, color: '#a3a3a3' }; break;
    case 3: dotState = { x: WORKERS[1].x, y: WORKERS[1].y, opacity: 1, color: '#a3a3a3' }; break; // Fail W2
    case 4: dotState = { x: 0, y: 0, opacity: 1, color: '#a3a3a3' }; break; // Bounce back
    case 5: dotState = { x: WORKERS[1].x, y: WORKERS[1].y, opacity: 1, color: WORKERS[1].color }; break; // Success W2
    case 6: dotState = { x: 0, y: 0, opacity: 0, color: WORKERS[2].color }; break;
    case 7: dotState = { x: WORKERS[2].x, y: WORKERS[2].y, opacity: 1, color: WORKERS[2].color }; break; // Success W3
  }

  return (
    <div className="relative w-24 h-24 flex items-center justify-center">
      {/* Central Orchestrator */}
      <div className="absolute w-6 h-6 bg-white border border-black z-10 rounded-full" />
      
      {/* Workers */}
      {WORKERS.map((worker) => (
        <AnimationShape
          key={worker.id}
          type={worker.type}
          color={worker.color}
          className="w-4 h-4 z-10"
          animate={{ x: worker.x, y: worker.y }}
        />
      ))}

      {/* Task Dot */}
      <motion.div
        className="absolute w-2 h-2 z-20 rounded-full"
        animate={{
          x: dotState.x,
          y: dotState.y,
          opacity: dotState.opacity,
          backgroundColor: dotState.color,
        }}
        transition={{ duration: 0.4 }}
      />
    </div>
  );
}
