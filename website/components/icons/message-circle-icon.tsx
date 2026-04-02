import { forwardRef, useImperativeHandle } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const MessageCircleIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();

    const start = async () => {
      animate(".message-path", { pathLength: 0, opacity: 0 }, { duration: 0 });
      await animate(
        ".message-path",
        { pathLength: [0, 1], opacity: [0, 1] },
        { duration: 0.6, ease: "easeInOut" },
      );
      animate(
        ".message-path",
        { scale: [1, 1.05, 1] },
        { duration: 0.3, ease: "easeOut" },
      );
    };

    const stop = () => {
      animate(
        ".message-path",
        { pathLength: 1, opacity: 1, scale: 1 },
        { duration: 0.2 },
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
        viewBox="0 0 24 24"
        fill="none"
        stroke={color}
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`cursor-pointer ${className}`}
        style={{ overflow: "visible" }}
      >
        <motion.path
          className="message-path"
          d="M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719"
          initial={{ pathLength: 1, opacity: 1 }}
          style={{ transformOrigin: "center" }}
        />
      </motion.svg>
    );
  },
);

MessageCircleIcon.displayName = "MessageCircleIcon";
export default MessageCircleIcon;
