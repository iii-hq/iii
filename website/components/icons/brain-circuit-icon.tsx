import { forwardRef, useImperativeHandle, useCallback } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const BrainCircuitIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const startAnimation = useCallback(() => {
      animate(
        ".brain-outline",
        { opacity: [1, 0.7, 1] },
        { duration: 2, repeat: Infinity, ease: "easeInOut" },
      );
      animate(
        ".circuit-line",
        { pathLength: [0, 1], opacity: [0, 1] },
        { duration: 0.6, ease: "easeOut" },
      );
      const terminals = [".terminal-1", ".terminal-2", ".terminal-3", ".terminal-4"];
      terminals.forEach((selector, index) => {
        animate(
          selector,
          { scale: [1, 1.5, 1], opacity: [0.5, 1, 0.5] },
          {
            duration: 0.8,
            delay: 0.2 + index * 0.15,
            repeat: Infinity,
            repeatDelay: 1,
            ease: "easeInOut",
          },
        );
      });
    }, [animate]);

    const stopAnimation = useCallback(() => {
      animate(".brain-outline", { opacity: 1 }, { duration: 0.3 });
      animate(".circuit-line", { pathLength: 1, opacity: 1 }, { duration: 0.3 });
      animate(".terminal", { scale: 1, opacity: 1 }, { duration: 0.3 });
    }, [animate]);

    useImperativeHandle(ref, () => ({
      startAnimation,
      stopAnimation,
    }));

    return (
      <div
        className={`relative flex items-center justify-center ${className}`}
        onMouseEnter={startAnimation}
        onMouseLeave={stopAnimation}
      >
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
        >
          <motion.path
            className="brain-outline"
            d="M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z"
          />
          <motion.path className="circuit-line" d="M9 13a4.5 4.5 0 0 0 3-4" />
          <motion.path className="brain-outline" d="M6.003 5.125A3 3 0 0 0 6.401 6.5" />
          <motion.path className="brain-outline" d="M3.477 10.896a4 4 0 0 1 .585-.396" />
          <motion.path className="brain-outline" d="M6 18a4 4 0 0 1-1.967-.516" />
          <motion.path className="circuit-line" d="M12 13h4" />
          <motion.path className="circuit-line" d="M12 18h6a2 2 0 0 1 2 2v1" />
          <motion.path className="circuit-line" d="M12 8h8" />
          <motion.path className="circuit-line" d="M16 8V5a2 2 0 0 1 2-2" />
          <motion.circle className="terminal terminal-1" cx="16" cy="13" r=".5" />
          <motion.circle className="terminal terminal-2" cx="18" cy="3" r=".5" />
          <motion.circle className="terminal terminal-3" cx="20" cy="21" r=".5" />
          <motion.circle className="terminal terminal-4" cx="20" cy="8" r=".5" />
        </motion.svg>
      </div>
    );
  },
);

BrainCircuitIcon.displayName = "BrainCircuitIcon";
export default BrainCircuitIcon;
