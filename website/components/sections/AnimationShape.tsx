import { motion, HTMLMotionProps } from 'motion/react';

export type ShapeType = 'circle' | 'square' | 'diamond';

interface AnimationShapeProps extends Omit<HTMLMotionProps<"div">, "animate"> {
  type: ShapeType;
  color: string;
  animate?: any;
}

export function AnimationShape({ type, color, className = '', animate, ...props }: AnimationShapeProps) {
  let shapeClasses = `absolute border border-black ${className}`;
  if (type === 'circle') shapeClasses += " rounded-full";
  else if (type === 'diamond') shapeClasses += " rotate-45 rounded-sm";
  else if (type === 'square') shapeClasses += " rounded-sm";

  return (
    <motion.div
      className={shapeClasses}
      animate={{
        backgroundColor: color,
        rotate: type === 'diamond' ? 45 : 0,
        borderRadius: type === 'circle' ? '50%' : '2px',
        ...animate
      }}
      {...props}
    />
  );
}
