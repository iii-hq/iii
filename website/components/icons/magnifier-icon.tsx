import { forwardRef, useImperativeHandle } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const MagnifierIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = async () => {
      await animate(
        ".magnifier-group",
        {
          x: [0, 1, 0, -1, 0],
          y: [0, -1, -2, -1, 0],
          rotate: [0, -5, 5, -5, 0],
        },
        { duration: 1, ease: "easeInOut" },
      );
    };

    const stop = () => {
      animate(
        ".magnifier-group",
        { x: 0, y: 0, rotate: 0 },
        { duration: 0.2, ease: "easeOut" },
      );
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
        viewBox="0 0 32 32"
        fill="none"
        stroke={color}
        strokeWidth={strokeWidth}
        strokeMiterlimit="10"
        className={`cursor-pointer ${className}`}
        style={{ overflow: "visible" }}
      >
        <motion.g
          className="magnifier-group"
          style={{ transformOrigin: "13px 13px", transformBox: "fill-box" }}
        >
          <motion.path d="m21.393,18.565l7.021,7.021c.781.781.781,2.047,0,2.828h0c-.781.781-2.047.781-2.828,0l-7.021-7.021" />
          <motion.circle cx="13" cy="13" r="10" strokeLinecap="square" />
        </motion.g>
      </motion.svg>
    );
  },
);

MagnifierIcon.displayName = "MagnifierIcon";
export default MagnifierIcon;
