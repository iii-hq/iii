import { forwardRef, useImperativeHandle } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const ClockIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = async () => {
      await animate(".clock-hands", { rotate: 360 }, { duration: 1, ease: "easeInOut" });
    };

    const stop = async () => {
      await animate(".clock-hands", { rotate: 0 }, { duration: 1, ease: "easeInOut" });
    };

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
        className={`${className} cursor-pointer`}
        onHoverStart={start}
        onHoverEnd={stop}
      >
        <motion.path stroke="none" d="M0 0h24v24H0z" fill="none" />
        <motion.path d="M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0" className="clock-body" />
        <motion.path d="M12 7v5l3 3" className="clock-hands" />
      </motion.svg>
    );
  },
);

ClockIcon.displayName = "ClockIcon";
export default ClockIcon;
