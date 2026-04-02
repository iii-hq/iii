import { forwardRef, useImperativeHandle, useCallback } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const RadioIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = useCallback(async () => {
      animate(
        ".center",
        { scale: [1, 1.3, 1] },
        { duration: 0.5, ease: "easeInOut" },
      );
      animate(
        ".wave-inner",
        { scale: [1, 1.15, 1], opacity: [1, 0.6, 1] },
        { duration: 0.6, ease: "easeInOut", delay: 0.1 },
      );
      await animate(
        ".wave-outer",
        { scale: [1, 1.2, 1], opacity: [1, 0.4, 1] },
        { duration: 0.7, ease: "easeInOut", delay: 0.2 },
      );
    }, [animate]);

    const stop = useCallback(() => {
      animate(
        ".center, .wave-inner, .wave-outer",
        { scale: 1, opacity: 1 },
        { duration: 0.2, ease: "easeInOut" },
      );
    }, [animate]);

    useImperativeHandle(ref, () => ({
      startAnimation: start,
      stopAnimation: stop,
    }));

    return (
      <motion.svg
        ref={scope}
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke={color}
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`cursor-pointer ${className}`}
        onHoverStart={start}
        onHoverEnd={stop}
      >
        <motion.g className="wave-inner">
          <path d="M16.247 7.761a6 6 0 0 1 0 8.478" style={{ transformOrigin: "12px 12px" }} />
          <path d="M7.753 16.239a6 6 0 0 1 0-8.478" style={{ transformOrigin: "12px 12px" }} />
        </motion.g>
        <motion.g className="wave-outer">
          <path d="M19.075 4.933a10 10 0 0 1 0 14.134" style={{ transformOrigin: "12px 12px" }} />
          <path d="M4.925 19.067a10 10 0 0 1 0-14.134" style={{ transformOrigin: "12px 12px" }} />
        </motion.g>
        <motion.circle className="center" cx="12" cy="12" r="2" style={{ transformOrigin: "12px 12px" }} />
      </motion.svg>
    );
  },
);

RadioIcon.displayName = "RadioIcon";
export default RadioIcon;
