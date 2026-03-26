import { motion } from 'motion/react';
import { AnimationShape, ShapeType } from './AnimationShape';

const CENTER_COLOR = 'var(--color-accent)';

const SHAPES: {
  id: number;
  color: string;
  type: ShapeType;
  x: number;
  y: number;
}[] = [
  { id: 0, color: '#42e7e7', type: 'circle', x: -40, y: 0 }, // Blue
  { id: 1, color: '#f3943d', type: 'square', x: 0, y: -40 }, // Orange
  { id: 2, color: '#ef2e61', type: 'diamond', x: 40, y: 0 }, // Red
];

function gradientFrame(angle: number, from: string, to: string, split: number) {
  return `linear-gradient(${angle}deg, ${from} ${split}%, ${to} ${split}%)`;
}

function pairFrames(
  from: string,
  to: string,
  outboundAngle: number,
  returnAngle: number,
) {
  return [
    gradientFrame(outboundAngle, from, to, 100),
    gradientFrame(outboundAngle, from, to, 100),
    gradientFrame(outboundAngle, from, to, 100),
    gradientFrame(outboundAngle, from, to, 50),
    gradientFrame(outboundAngle, from, to, 0),
    gradientFrame(outboundAngle, from, to, 0),
    gradientFrame(outboundAngle, from, to, 0),
    gradientFrame(returnAngle, to, from, 50),
    gradientFrame(returnAngle, from, to, 100),
    gradientFrame(returnAngle, from, to, 100),
  ];
}

export function ExecutionModelAnimation() {
  const circleColor = SHAPES[0].color;
  const squareColor = SHAPES[1].color;
  const diamondColor = SHAPES[2].color;

  const xFrames = [
    -40, -40, 0, 0, 0, 0, 0, 0, -40, -40, 0, 0, 0, 0, 40, 40, 0, 0, 0, 0, 40,
    40, 0, 0, -40, -40, 0, 0, 40, 40,
  ];
  const yFrames = [
    0, 0, 0, 0, -40, -40, 0, 0, 0, 0, -40, -40, 0, 0, 0, 0, 0, 0, -40, -40, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0,
  ];
  const bgFrames = [
    // Circle <-> Square: outbound to top, return to left
    ...pairFrames(circleColor, squareColor, 0, 270),
    // Square <-> Diamond: outbound to right, return to top
    ...pairFrames(squareColor, diamondColor, 90, 0),
    // Diamond <-> Circle: outbound to left, return to right
    ...pairFrames(diamondColor, circleColor, 270, 90),
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
      <div
        className="absolute w-7 h-7 sm:w-8 sm:h-8 border border-black z-20 rounded-sm"
        style={{
          backgroundColor: CENTER_COLOR,
          filter: 'blur(0.2px)',
          boxShadow: '0 0 4px rgba(0, 0, 0, 0.2)',
        }}
      />

      {/* Shapes */}
      {SHAPES.map((shape) => (
        <AnimationShape
          key={`shape-${shape.id}`}
          type={shape.type}
          color={shape.color}
          className="w-4 h-4 sm:w-[22px] sm:h-[22px] z-20 border border-black"
          animate={{ x: shape.x, y: shape.y }}
        />
      ))}

      {/* Task Dot */}
      <motion.div
        className="absolute w-3 h-3 z-30 rounded-full"
        style={{
          filter: 'blur(0.4px)',
          boxShadow: '0 0 6px rgba(255, 255, 255, 0.35)',
        }}
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
