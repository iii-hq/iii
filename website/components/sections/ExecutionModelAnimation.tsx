import { motion } from 'motion/react';
import { AnimationShape, ShapeType } from './AnimationShape';

const SHAPES: {
  id: number;
  color: string;
  type: ShapeType;
  x: number;
  y: number;
}[] = [
  { id: 0, color: '#42e7e7', type: 'circle', x: -35, y: 0 }, // Blue
  { id: 1, color: '#f3943d', type: 'square', x: 0, y: -35 }, // Orange
  { id: 2, color: '#ef2e61', type: 'diamond', x: 35, y: 0 }, // Red
];

function pairFrames(from: string, to: string) {
  return [
    `linear-gradient(90deg, ${from} 100%, ${to} 100%)`,
    `linear-gradient(90deg, ${from} 100%, ${to} 100%)`,
    `linear-gradient(90deg, ${from} 100%, ${to} 100%)`,
    `linear-gradient(90deg, ${from} 50%, ${to} 50%)`,
    `linear-gradient(90deg, ${from} 0%, ${to} 0%)`,
    `linear-gradient(90deg, ${from} 0%, ${to} 0%)`,
    `linear-gradient(90deg, ${from} 0%, ${to} 0%)`,
    `linear-gradient(90deg, ${from} 50%, ${to} 50%)`,
    `linear-gradient(90deg, ${from} 100%, ${to} 100%)`,
    `linear-gradient(90deg, ${from} 100%, ${to} 100%)`,
  ];
}

export function ExecutionModelAnimation() {
  const circleColor = SHAPES[0].color;
  const squareColor = SHAPES[1].color;
  const diamondColor = SHAPES[2].color;

  const xFrames = [
    -35, -35, 0, 0, 0, 0, 0, 0, -35, -35, 0, 0, 0, 0, 35, 35, 0, 0, 0, 0, 35,
    35, 0, 0, -35, -35, 0, 0, 35, 35,
  ];
  const yFrames = [
    0, 0, 0, 0, -35, -35, 0, 0, 0, 0, -35, -35, 0, 0, 0, 0, 0, 0, -35, -35, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0,
  ];
  const bgFrames = [
    ...pairFrames(circleColor, squareColor),
    ...pairFrames(squareColor, diamondColor),
    ...pairFrames(diamondColor, circleColor),
  ];
  const opacityFrames = [
    0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1,
    1, 1, 1, 1, 0,
  ];
  const times = [
    0, 0.0167, 0.0667, 0.1, 0.15, 0.1833, 0.2333, 0.2667, 0.3167, 0.3333,
    0.3334, 0.35, 0.4, 0.4333, 0.4833, 0.5167, 0.5667, 0.6, 0.65, 0.6667,
    0.6668, 0.6833, 0.7333, 0.7667, 0.8167, 0.85, 0.9, 0.9333, 0.9833, 1,
  ];

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
          className="w-5 h-5 z-20"
          animate={{ x: shape.x, y: shape.y }}
        />
      ))}

      {/* Task Dot */}
      <motion.div
        className="absolute w-3 h-3 z-30 rounded-full"
        animate={{
          x: xFrames,
          y: yFrames,
          background: bgFrames,
          opacity: opacityFrames,
        }}
        transition={{
          duration: 12,
          repeat: Infinity,
          ease: 'linear',
          times: times,
        }}
      />
    </div>
  );
}
