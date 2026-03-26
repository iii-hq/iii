import { motion } from 'motion/react';
import { AnimationShape, ShapeType } from './AnimationShape';

const SHAPES: { id: number; color: string; type: ShapeType; x: number; y: number }[] = [
  { id: 0, color: '#009246', type: 'circle', x: -35, y: 0 }, // Green
  { id: 1, color: '#ffffff', type: 'square', x: 0, y: -35 }, // White
  { id: 2, color: '#ce2b37', type: 'diamond', x: 35, y: 0 },  // Red
];

const DOTS = [
  { id: 0, target: 0, delay: 0, duration: 2 },
  { id: 1, target: 1, delay: 0.6, duration: 2 },
  { id: 2, target: 2, delay: 1.2, duration: 2 },
  { id: 3, target: 0, delay: 1.0, duration: 2 },
  { id: 4, target: 1, delay: 1.6, duration: 2 },
  { id: 5, target: 2, delay: 0.2, duration: 2 },
];

export function ExecutionModelAnimation() {
  return (
    <div className="relative w-24 h-24 flex items-center justify-center">
      {/* Central Box */}
      <div className="absolute w-8 h-8 bg-white border border-black z-20 rounded-sm" />

      {/* Shapes */}
      {SHAPES.map((shape) => (
        <AnimationShape
          key={`shape-${shape.id}`}
          type={shape.type}
          color={shape.color}
          className="w-4 h-4 z-20"
          animate={{ x: shape.x, y: shape.y }}
        />
      ))}

      {/* Task Dots */}
      {DOTS.map((dot) => {
        const targetShape = SHAPES[dot.target];
        return (
          <motion.div
            key={`dot-${dot.id}`}
            className="absolute w-2 h-2 bg-white border border-black z-10 rounded-full"
            animate={{
              x: [0, targetShape.x, 0],
              y: [0, targetShape.y, 0],
            }}
            transition={{
              duration: dot.duration,
              repeat: Infinity,
              ease: "easeInOut",
              delay: dot.delay,
            }}
          />
        );
      })}
    </div>
  );
}
