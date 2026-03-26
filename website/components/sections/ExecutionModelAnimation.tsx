import { motion } from 'motion/react';
import { useEffect, useState } from 'react';

export function ExecutionModelAnimation() {
  const [step, setStep] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setStep((s) => (s + 1) % 8);
    }, 800);
    return () => clearInterval(interval);
  }, []);

  // Workers positions
  const w1 = { x: 0, y: -25 };
  const w2 = { x: -25, y: 20 };
  const w3 = { x: 25, y: 20 };

  let dotState = { x: 0, y: 0, opacity: 0, color: '#000000' };
  
  switch (step) {
    case 0: dotState = { x: 0, y: 0, opacity: 0, color: '#3b82f6' }; break;
    case 1: dotState = { x: w1.x, y: w1.y, opacity: 1, color: '#3b82f6' }; break; // Success W1
    case 2: dotState = { x: 0, y: 0, opacity: 0, color: '#ef4444' }; break;
    case 3: dotState = { x: w2.x, y: w2.y, opacity: 1, color: '#ef4444' }; break; // Fail W2
    case 4: dotState = { x: 0, y: 0, opacity: 1, color: '#ef4444' }; break; // Bounce back
    case 5: dotState = { x: w2.x, y: w2.y, opacity: 1, color: '#10b981' }; break; // Success W2
    case 6: dotState = { x: 0, y: 0, opacity: 0, color: '#3b82f6' }; break;
    case 7: dotState = { x: w3.x, y: w3.y, opacity: 1, color: '#3b82f6' }; break; // Success W3
  }

  return (
    <div className="relative w-24 h-24 flex items-center justify-center">
      {/* Central Orchestrator */}
      <div className="absolute w-6 h-6 bg-white border border-black z-10 rounded-full" />
      
      {/* Workers */}
      <div className="absolute w-4 h-4 bg-white border border-black z-10 rounded-sm" style={{ transform: `translate(${w1.x}px, ${w1.y}px)` }} />
      <div className="absolute w-4 h-4 bg-white border border-black z-10 rounded-sm" style={{ transform: `translate(${w2.x}px, ${w2.y}px)` }} />
      <div className="absolute w-4 h-4 bg-white border border-black z-10 rounded-sm" style={{ transform: `translate(${w3.x}px, ${w3.y}px)` }} />

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
