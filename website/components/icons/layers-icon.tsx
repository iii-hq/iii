import { forwardRef, useImperativeHandle } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const LayersIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = async () => {
      await animate(".top-block", { x: -20 }, { duration: 0.4, ease: [0.4, 0, 0.2, 1] });
    };

    const stop = async () => {
      await animate(".top-block", { x: 0 }, { duration: 0.4, ease: [0.4, 0, 0.2, 1] });
    };

    useImperativeHandle(ref, () => ({
      startAnimation: start,
      stopAnimation: stop,
    }));

    return (
      <motion.svg
        ref={scope}
        onHoverStart={start}
        onHoverEnd={stop}
        width={size}
        height={size}
        viewBox="0 0 120 120"
        fill="none"
        strokeWidth={strokeWidth}
        xmlns="http://www.w3.org/2000/svg"
        className={`cursor-pointer ${className}`}
        style={{ overflow: "visible" }}
      >
        <motion.rect className="top-block" x="44" y="22" width="56" height="36" rx="10" fill={color} />
        <rect x="20" y="62" width="64" height="40" rx="12" fill={color} />
      </motion.svg>
    );
  },
);

LayersIcon.displayName = "LayersIcon";
export default LayersIcon;
