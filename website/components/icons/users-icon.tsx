import { forwardRef, useImperativeHandle, useCallback } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const UsersIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = useCallback(async () => {
      animate(
        ".user-primary",
        { y: -2, scale: 1.05 },
        { duration: 0.3, ease: "easeOut" },
      );
      animate(
        ".user-secondary",
        { x: 1, opacity: 0.8 },
        { duration: 0.3, ease: "easeOut" },
      );
    }, [animate]);

    const stop = useCallback(async () => {
      animate(
        ".user-primary",
        { y: 0, scale: 1 },
        { duration: 0.25, ease: "easeInOut" },
      );
      animate(
        ".user-secondary",
        { x: 0, opacity: 1 },
        { duration: 0.25, ease: "easeInOut" },
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
        <motion.g className="user-primary">
          <path d="M9 7m-4 0a4 4 0 1 0 8 0a4 4 0 1 0 -8 0" />
          <path d="M3 21v-2a4 4 0 0 1 4 -4h4a4 4 0 0 1 4 4v2" />
        </motion.g>
        <motion.g className="user-secondary">
          <path d="M16 3.13a4 4 0 0 1 0 7.75" />
          <path d="M21 21v-2a4 4 0 0 0 -3 -3.85" />
        </motion.g>
      </motion.svg>
    );
  },
);

UsersIcon.displayName = "UsersIcon";
export default UsersIcon;
