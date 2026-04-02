import { forwardRef, useImperativeHandle } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const CodeIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = async () => {
      animate(".left-bracket", { x: -3 }, { duration: 0.3, ease: "easeOut" });
      animate(".right-bracket", { x: 3 }, { duration: 0.3, ease: "easeOut" });
    };

    const stop = () => {
      animate(".left-bracket", { x: 0 }, { duration: 0.2, ease: "easeInOut" });
      animate(".right-bracket", { x: 0 }, { duration: 0.2, ease: "easeInOut" });
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
        <motion.path className="right-bracket" d="m16 18 6-6-6-6" />
        <motion.path className="left-bracket" d="m8 6-6 6 6 6" />
      </motion.svg>
    );
  },
);

CodeIcon.displayName = "CodeIcon";
export default CodeIcon;
