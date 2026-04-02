import { forwardRef, useImperativeHandle, useCallback } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const TerminalIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = useCallback(async () => {
      animate(
        ".cursor-line",
        { opacity: [1, 0, 1, 0, 1] },
        { duration: 0.8, ease: "easeInOut" },
      );
      animate(
        ".terminal-chevron",
        { x: [0, 2, 0] },
        { duration: 0.3, ease: "easeInOut" },
      );
    }, [animate]);

    const stop = useCallback(() => {
      animate(".cursor-line", { opacity: 1 }, { duration: 0.2, ease: "easeOut" });
      animate(".terminal-chevron", { x: 0 }, { duration: 0.2, ease: "easeOut" });
    }, [animate]);

    useImperativeHandle(ref, () => ({
      startAnimation: start,
      stopAnimation: stop,
    }));

    return (
      <motion.svg
        ref={scope}
        onHoverStart={start}
        onHoverEnd={stop}
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
      >
        <path stroke="none" d="M0 0h24v24H0z" fill="none" />
        <motion.path className="terminal-chevron" d="M5 7l5 5l-5 5" />
        <motion.path className="cursor-line" d="M12 19l7 0" />
      </motion.svg>
    );
  },
);

TerminalIcon.displayName = "TerminalIcon";
export default TerminalIcon;
