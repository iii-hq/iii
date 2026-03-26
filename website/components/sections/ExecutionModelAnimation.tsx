import { motion } from 'motion/react';
import { AnimationShape, ShapeType } from './AnimationShape';

const SHAPES: { id: number; color: string; type: ShapeType; x: number; y: number }[] = [
  { id: 0, color: '#2f7fff', type: 'circle', x: -35, y: 0 }, // Blue
  { id: 1, color: '#009246', type: 'square', x: 0, y: -35 }, // Green
  { id: 2, color: '#ce2b37', type: 'diamond', x: 35, y: 0 },  // Red
];

export function ExecutionModelAnimation() {
  const xFrames = [-35,-35,0,0,0,0,0,0,-35,-35,0,0,0,0,35,35,0,0,0,0,35,35,0,0,-35,-35,0,0,35,35];
  const yFrames = [0,0,0,0,-35,-35,0,0,0,0,-35,-35,0,0,0,0,0,0,-35,-35,0,0,0,0,0,0,0,0,0,0];
  const bgFrames = ["linear-gradient(90deg, #2f7fff 100%, #009246 100%)","linear-gradient(90deg, #2f7fff 100%, #009246 100%)","linear-gradient(90deg, #2f7fff 100%, #009246 100%)","linear-gradient(90deg, #2f7fff 50%, #009246 50%)","linear-gradient(90deg, #2f7fff 0%, #009246 0%)","linear-gradient(90deg, #2f7fff 0%, #009246 0%)","linear-gradient(90deg, #2f7fff 0%, #009246 0%)","linear-gradient(90deg, #2f7fff 50%, #009246 50%)","linear-gradient(90deg, #2f7fff 100%, #009246 100%)","linear-gradient(90deg, #2f7fff 100%, #009246 100%)","linear-gradient(90deg, #009246 100%, #ce2b37 100%)","linear-gradient(90deg, #009246 100%, #ce2b37 100%)","linear-gradient(90deg, #009246 100%, #ce2b37 100%)","linear-gradient(90deg, #009246 50%, #ce2b37 50%)","linear-gradient(90deg, #009246 0%, #ce2b37 0%)","linear-gradient(90deg, #009246 0%, #ce2b37 0%)","linear-gradient(90deg, #009246 0%, #ce2b37 0%)","linear-gradient(90deg, #009246 50%, #ce2b37 50%)","linear-gradient(90deg, #009246 100%, #ce2b37 100%)","linear-gradient(90deg, #009246 100%, #ce2b37 100%)","linear-gradient(90deg, #ce2b37 100%, #2f7fff 100%)","linear-gradient(90deg, #ce2b37 100%, #2f7fff 100%)","linear-gradient(90deg, #ce2b37 100%, #2f7fff 100%)","linear-gradient(90deg, #ce2b37 50%, #2f7fff 50%)","linear-gradient(90deg, #ce2b37 0%, #2f7fff 0%)","linear-gradient(90deg, #ce2b37 0%, #2f7fff 0%)","linear-gradient(90deg, #ce2b37 0%, #2f7fff 0%)","linear-gradient(90deg, #ce2b37 50%, #2f7fff 50%)","linear-gradient(90deg, #ce2b37 100%, #2f7fff 100%)","linear-gradient(90deg, #ce2b37 100%, #2f7fff 100%)"];
  const opacityFrames = [0,1,1,1,1,1,1,1,1,0,0,1,1,1,1,1,1,1,1,0,0,1,1,1,1,1,1,1,1,0];
  const times = [0,0.0167,0.0667,0.1,0.15,0.1833,0.2333,0.2667,0.3167,0.3333,0.3334,0.35,0.4,0.4333,0.4833,0.5167,0.5667,0.6,0.65,0.6667,0.6668,0.6833,0.7333,0.7667,0.8167,0.85,0.9,0.9333,0.9833,1];

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

      {/* Task Dot */}
      <motion.div
        className="absolute w-2 h-2 border border-black z-30 rounded-full"
        animate={{
          x: xFrames,
          y: yFrames,
          background: bgFrames,
          opacity: opacityFrames,
        }}
        transition={{
          duration: 12,
          repeat: Infinity,
          ease: "linear",
          times: times,
        }}
      />
    </div>
  );
}
