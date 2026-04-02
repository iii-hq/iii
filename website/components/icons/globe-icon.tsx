import { forwardRef, useImperativeHandle, useRef } from "react";
import type { AnimatedIconHandle, AnimatedIconProps } from "./types";
import { motion, useAnimate } from "motion/react";

const GlobeIcon = forwardRef<AnimatedIconHandle, AnimatedIconProps>(
  (
    { size = 24, color = "currentColor", strokeWidth = 2, className = "" },
    ref,
  ) => {
    const [scope, animate] = useAnimate();
    const animationControls = useRef<Array<ReturnType<typeof animate>>>([]);

    const start = async () => {
      animationControls.current.forEach((control) => control.stop());
      animationControls.current = [];
      animationControls.current.push(
        animate(
          ".globe-circle",
          { rotate: 360 },
          { duration: 2, ease: "linear", repeat: Infinity },
        ),
      );
    };

    const stop = () => {
      animationControls.current.forEach((control) => control.stop());
      animationControls.current = [];
      animate(".globe-circle", { rotate: 0 }, { duration: 0.5 });
    };

    useImperativeHandle(ref, () => ({
      startAnimation: start,
      stopAnimation: stop,
    }));

    return (
      <motion.div
        ref={scope}
        className={`inline-flex cursor-pointer ${className}`}
        onHoverStart={start}
        onHoverEnd={stop}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width={size}
          height={size}
          viewBox="0 0 48 48"
          fill="none"
          stroke={color}
          strokeWidth={strokeWidth}
          strokeMiterlimit="10"
          strokeLinecap="square"
        >
          <motion.g className="globe-circle" style={{ transformOrigin: "23px 19px" }}>
            <motion.path
              className="globe-continent-1"
              d="M36.6225 22.1264C34.6145 19.2959 32.3651 15.7913 28.4377 17.3428C24.4307 18.9257 30.0493 23.15 25.2064 26.9189C22.1135 29.3259 22.8515 31.6477 23.9478 33"
            />
            <motion.path
              className="globe-continent-2"
              d="M14 30L15.336 28.0984C16.3999 26.5841 16.557 24.5077 15.7357 22.8151L15.5751 22.4842C14.5131 20.2955 15.1651 17.5604 17.0607 16.253L17.3292 16.0677C18.2109 15.4596 18.808 14.4478 18.9613 13.3023C19.1316 12.0291 18.7338 10.7433 17.8962 9.85981L15.3599 7.24048"
            />
            <motion.path
              className="globe-continent-3"
              d="M23.0628 5C22.3771 9.64991 27.3946 14.948 33.7332 10.0381"
            />
            <motion.path
              className="globe-outline"
              d="M23 33C30.732 33 37 26.732 37 19C37 11.268 30.732 5 23 5C15.268 5 9 11.268 9 19C9 26.732 15.268 33 23 33Z"
            />
          </motion.g>
          <motion.path className="globe-stand-stem" d="M23 43V38" />
          <motion.path className="globe-stand-base" d="M16 43H30" />
          <motion.path
            className="globe-axis"
            d="M38 3.99994L36.435 5.56491C43.855 12.9849 43.855 25.015 36.435 32.435C29.0151 39.8549 16.9849 39.8549 9.56497 32.435L7.99997 34"
          />
        </svg>
      </motion.div>
    );
  },
);

GlobeIcon.displayName = "GlobeIcon";
export default GlobeIcon;
