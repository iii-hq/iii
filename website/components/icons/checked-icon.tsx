import { forwardRef, useImperativeHandle } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const CheckedIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = async () => {
      await animate("svg", { scale: 1.1 }, { duration: 0.1, ease: "easeInOut" });
      await animate(".check-icon", { pathLength: 0 }, { duration: 0.1, ease: "easeInOut" });
      await animate(".check-icon", { pathLength: 1 }, { duration: 0.4, ease: "easeInOut" });
      await animate("svg", { scale: 1 }, { duration: 0.2, ease: "easeInOut" });
    };

    const stop = () => {
      animate("svg", { scale: 1 }, { duration: 0.2 });
      animate(".check-icon", { pathLength: 1 }, { duration: 0.2 });
    };

    useImperativeHandle(ref, () => ({
      startAnimation: start,
      stopAnimation: stop,
    }));

    return (
      <motion.div ref={scope} onHoverStart={start} onHoverEnd={stop}>
        <motion.svg
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
          <motion.path stroke="none" d="M0 0h24v24H0z" fill="none" />
          <motion.path d="M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0 -18 0" />
          <motion.path d="M9 12l2 2l4 -4" className="check-icon" />
        </motion.svg>
      </motion.div>
    );
  },
);

CheckedIcon.displayName = "CheckedIcon";
export default CheckedIcon;
