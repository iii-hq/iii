import { forwardRef, useImperativeHandle, useCallback } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const XIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = useCallback(async () => {
      animate(
        ".x-line-1",
        { rotate: 15, scale: 1.1 },
        { duration: 0.2, ease: "easeOut" },
      );
      animate(
        ".x-line-2",
        { rotate: -15, scale: 1.1 },
        { duration: 0.2, ease: "easeOut" },
      );
    }, [animate]);

    const stop = useCallback(async () => {
      animate(
        ".x-line-1",
        { rotate: 0, scale: 1 },
        { duration: 0.2, ease: "easeInOut" },
      );
      animate(
        ".x-line-2",
        { rotate: 0, scale: 1 },
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
        <path stroke="none" d="M0 0h24v24H0z" fill="none" />
        <motion.path d="M18 6l-12 12" className="x-line-1" style={{ transformOrigin: "50% 50%" }} />
        <motion.path d="M6 6l12 12" className="x-line-2" style={{ transformOrigin: "50% 50%" }} />
      </motion.svg>
    );
  },
);

XIcon.displayName = "XIcon";
export default XIcon;
