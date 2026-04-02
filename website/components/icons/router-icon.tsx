import { forwardRef, useImperativeHandle, useCallback } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const RouterIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = useCallback(async () => {
      animate(
        ".signal-inner",
        { scale: [1, 1.2, 1], opacity: [1, 0.6, 1] },
        { duration: 0.6, ease: "easeInOut" },
      );
      await animate(
        ".signal-outer",
        { scale: [1, 1.3, 1], opacity: [1, 0.4, 1] },
        { duration: 0.7, ease: "easeInOut", delay: 0.1 },
      );
      animate(
        ".indicator",
        { opacity: [1, 0.3, 1] },
        { duration: 0.3, ease: "easeInOut" },
      );
    }, [animate]);

    const stop = useCallback(() => {
      animate(
        ".signal-inner, .signal-outer, .indicator",
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
        <rect width="20" height="8" x="2" y="14" rx="2" />
        <motion.g className="indicator">
          <path d="M6.01 18H6" />
          <path d="M10.01 18H10" />
        </motion.g>
        <path d="M15 10v4" />
        <motion.path
          className="signal-inner"
          d="M17.84 7.17a4 4 0 0 0-5.66 0"
          style={{ transformOrigin: "15px 7px" }}
        />
        <motion.path
          className="signal-outer"
          d="M20.66 4.34a8 8 0 0 0-11.31 0"
          style={{ transformOrigin: "15px 4px" }}
        />
      </motion.svg>
    );
  },
);

RouterIcon.displayName = "RouterIcon";
export default RouterIcon;
