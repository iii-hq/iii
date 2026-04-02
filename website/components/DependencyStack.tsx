import React, { useEffect, useState, useRef } from "react";

interface Box {
  id: number;
  width: number;
  height: number;
  // Position in the xkcd dependency stack (center point for rendering)
  stackX: number;
  stackY: number;
  // Position in the final layout (center point)
  endX: number;
  endY: number;
  // Target size in final layout
  endWidth: number;
  endHeight: number;
  // Which container this box belongs to in end state: 'left' | 'right'
  endContainer: "left" | "right";
}

// Layout configuration for the final 3-box layout
const CONTAINER_WIDTH = 160; // Left and right container size
const CONTAINER_HEIGHT = 160;
const CENTER_BOX_SIZE = 80; // Smaller center "iii" box
const CONTAINER_GAP = 60; // Gap between containers (for dashed lines)

// Left container: fewer, larger boxes (e.g., 3x3 = 9 boxes)
const LEFT_GRID_SIZE = 3;
const LEFT_BOX_SIZE = 42;
const LEFT_BOX_GAP = 8;

// Right container: more, smaller boxes (e.g., 5x5 = 25 boxes, but we'll use remaining boxes)
const RIGHT_BOX_SIZE = 24;
const RIGHT_BOX_GAP = 4;

// Variant configurations
export type DependencyStackVariant = "fullscreen" | "corner" | "splash";

interface DependencyStackProps {
  /** Visual variant of the component */
  variant?: DependencyStackVariant;
  /** Override opacity (0-1) */
  opacity?: number;
  /** Custom class name for the container */
  className?: string;
  /** Whether to animate based on scroll (true) or use fixed progress */
  animateOnScroll?: boolean;
  /** Fixed progress value (0-1) when animateOnScroll is false */
  progress?: number;
  /** Show the bracket at top */
  showBracket?: boolean;
  /** Scale multiplier for the entire visualization */
  scale?: number;
}

// Create boxes that form the dependency stack, then rearrange into the 3-container layout
// Using BOTTOM-LEFT positioning: (left, bottom) with width extending right, height extending UP
// In SVG, Y increases downward, so "up" means smaller Y values
// bottom = Y coordinate of bottom edge, top = bottom - height
const createBoxes = (): Box[] => {
  const boxes: Box[] = [];

  // Ground level (bottom of the structure)
  const GROUND = 360;

  // Positions use: left (x of left edge), bottom (y of bottom edge), w (width), h (height)
  // Block stacks upward, so top of block = bottom - height
  const stackPositions = [
    // === BOTTOM BASE PLATFORMS (3 wide horizontal layers) ===
    // Ground level platform
    { left: -170, bottom: GROUND, w: 340, h: 28 },
    // Stacks on top: bottom = previous.bottom - previous.h
    { left: -150, bottom: GROUND - 28, w: 300, h: 26 },
    { left: -130, bottom: GROUND - 28 - 26, w: 260, h: 24 },

    // === PEDESTAL ===
    { left: -70, bottom: GROUND - 28 - 26 - 24, w: 140, h: 30 },

    // === MAIN FOUNDATION BLOCK (large) — BELOW NEBRASKA ===
    { left: -100, bottom: GROUND - 28 - 26 - 24 - 30, w: 200, h: 110 },
    // Top of this block = 360 - 28 - 26 - 24 - 30 - 110 = 142

    // === THE CRITICAL "NEBRASKA" BLOCK (tall and skinny) ===
    // Rests on top of foundation block, so bottom = 142
    { left: 10, bottom: 142, w: 28, h: 55 },
    // Top of Nebraska = 142 - 55 = 87

    // === COMPANION BLOCK (to the left of Nebraska, same level) ===
    { left: -55, bottom: 142, w: 35, h: 55 },

    // === MIDDLE SUPPORT LAYER — ABOVE NEBRASKA ===
    // This wide block rests on Nebraska and companion (and hangs over edges)
    { left: -85, bottom: 87, w: 175, h: 50 },
    // Top = 87 - 50 = 37

    // === SECOND MIDDLE LAYER ===
    { left: -70, bottom: 37, w: 145, h: 42 },
    // Top = 37 - 42 = -5

    // === LEFT TOWER (stacked vertically) ===
    { left: -90, bottom: -5, w: 42, h: 52 },
    { left: -88, bottom: -57, w: 36, h: 46 },
    { left: -84, bottom: -103, w: 30, h: 40 },
    { left: -82, bottom: -143, w: 26, h: 34 },
    { left: -80, bottom: -177, w: 22, h: 28 },
    { left: -78, bottom: -205, w: 18, h: 24 },

    // === MIDDLE TOWER (tallest) ===
    { left: -24, bottom: -5, w: 52, h: 65 },
    { left: -20, bottom: -70, w: 45, h: 55 },
    { left: -18, bottom: -125, w: 40, h: 50 },
    { left: -15, bottom: -175, w: 34, h: 45 },
    { left: -12, bottom: -220, w: 28, h: 40 },
    { left: -10, bottom: -260, w: 24, h: 35 },
    { left: -8, bottom: -295, w: 20, h: 30 },

    // === RIGHT TOWER ===
    { left: 50, bottom: -5, w: 48, h: 58 },
    { left: 54, bottom: -63, w: 42, h: 52 },
    { left: 56, bottom: -115, w: 36, h: 46 },
    { left: 58, bottom: -161, w: 30, h: 40 },
    { left: 60, bottom: -201, w: 26, h: 34 },

    // === SMALL TOP BLOCKS ===
    { left: -76, bottom: -229, w: 16, h: 20 },
    { left: -74, bottom: -249, w: 14, h: 18 },
    { left: -6, bottom: -325, w: 18, h: 26 },
    { left: -5, bottom: -351, w: 14, h: 22 },
    { left: 62, bottom: -235, w: 22, h: 28 },
    { left: 64, bottom: -263, w: 18, h: 24 },

    // === TINY ACCENT BLOCKS ===
    { left: -72, bottom: -267, w: 12, h: 14 },
    { left: -4, bottom: -373, w: 12, h: 18 },
    { left: 66, bottom: -287, w: 14, h: 20 },
  ];

  // Calculate total width of the 3-container layout
  const totalWidth = CONTAINER_WIDTH * 2 + CENTER_BOX_SIZE + CONTAINER_GAP * 2;
  const leftContainerCenterX = -totalWidth / 2 + CONTAINER_WIDTH / 2;
  const centerBoxCenterX = 0;
  const rightContainerCenterX = totalWidth / 2 - CONTAINER_WIDTH / 2;

  // Assign boxes to left (first 9) and right (remaining) containers
  const leftBoxCount = LEFT_GRID_SIZE * LEFT_GRID_SIZE; // 9 boxes for left

  for (let i = 0; i < stackPositions.length; i++) {
    const stack = stackPositions[i];
    const isLeft = i < leftBoxCount;

    // Convert bottom-left to center for consistent rendering
    const centerX = stack.left + stack.w / 2;
    const centerY = stack.bottom - stack.h / 2;

    let endX: number, endY: number, endWidth: number, endHeight: number;

    if (isLeft) {
      // Left container: 3x3 grid of larger boxes
      const gridRow = Math.floor(i / LEFT_GRID_SIZE);
      const gridCol = i % LEFT_GRID_SIZE;
      const gridWidth =
        LEFT_GRID_SIZE * LEFT_BOX_SIZE + (LEFT_GRID_SIZE - 1) * LEFT_BOX_GAP;
      const gridHeight = gridWidth;
      const startX = leftContainerCenterX - gridWidth / 2;
      const startY = -gridHeight / 2;

      endX =
        startX + gridCol * (LEFT_BOX_SIZE + LEFT_BOX_GAP) + LEFT_BOX_SIZE / 2;
      endY =
        startY + gridRow * (LEFT_BOX_SIZE + LEFT_BOX_GAP) + LEFT_BOX_SIZE / 2;
      endWidth = LEFT_BOX_SIZE;
      endHeight = LEFT_BOX_SIZE;
    } else {
      // Right container: dynamic grid of smaller boxes
      const rightIndex = i - leftBoxCount;
      const rightBoxCount = stackPositions.length - leftBoxCount;
      const rightGridCols = Math.ceil(Math.sqrt(rightBoxCount));
      const rightGridRows = Math.ceil(rightBoxCount / rightGridCols);

      const gridRow = Math.floor(rightIndex / rightGridCols);
      const gridCol = rightIndex % rightGridCols;
      const gridWidth =
        rightGridCols * RIGHT_BOX_SIZE + (rightGridCols - 1) * RIGHT_BOX_GAP;
      const gridHeight =
        rightGridRows * RIGHT_BOX_SIZE + (rightGridRows - 1) * RIGHT_BOX_GAP;
      const startX = rightContainerCenterX - gridWidth / 2;
      const startY = -gridHeight / 2;

      endX =
        startX +
        gridCol * (RIGHT_BOX_SIZE + RIGHT_BOX_GAP) +
        RIGHT_BOX_SIZE / 2;
      endY =
        startY +
        gridRow * (RIGHT_BOX_SIZE + RIGHT_BOX_GAP) +
        RIGHT_BOX_SIZE / 2;
      endWidth = RIGHT_BOX_SIZE;
      endHeight = RIGHT_BOX_SIZE;
    }

    boxes.push({
      id: i,
      width: stack.w,
      height: stack.h,
      stackX: centerX,
      stackY: centerY,
      endX,
      endY,
      endWidth,
      endHeight,
      endContainer: isLeft ? "left" : "right",
    });
  }

  return boxes;
};

// Variant style configurations
const variantStyles: Record<
  DependencyStackVariant,
  {
    containerClass: string;
    svgClass: string;
    svgStyle: React.CSSProperties;
    defaultOpacity: number;
    defaultShowBracket: boolean;
  }
> = {
  fullscreen: {
    containerClass: "fixed inset-0 w-screen h-screen",
    svgClass: "absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2",
    svgStyle: { width: "100vw", height: "100vh" },
    defaultOpacity: 0.15,
    defaultShowBracket: true,
  },
  corner: {
    containerClass: "fixed bottom-0 right-0 w-[35vw] h-[50vh]",
    svgClass: "w-full h-full",
    svgStyle: {},
    defaultOpacity: 0.25,
    defaultShowBracket: false,
  },
  splash: {
    containerClass: "relative w-full h-full min-h-[400px]",
    svgClass: "absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2",
    svgStyle: {
      width: "100%",
      height: "100%",
      maxWidth: "800px",
      maxHeight: "800px",
    },
    defaultOpacity: 0.25,
    defaultShowBracket: true,
  },
};

// Easing function for smoother animation
const easeInOutCubic = (t: number) =>
  t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;

export function DependencyStack({
  variant = "fullscreen",
  opacity,
  className = "",
  animateOnScroll = true,
  progress: fixedProgress = 0,
  showBracket,
  scale = 1,
}: DependencyStackProps) {
  const [scrollProgress, setScrollProgress] = useState(fixedProgress);
  const containerRef = useRef<HTMLDivElement>(null);
  const boxes = useRef(createBoxes()).current;

  const styles = variantStyles[variant];
  const finalOpacity = opacity ?? styles.defaultOpacity;
  const finalShowBracket = showBracket ?? styles.defaultShowBracket;

  useEffect(() => {
    if (!animateOnScroll) {
      setScrollProgress(fixedProgress);
      return;
    }

    const handleScroll = () => {
      const scrollTop = window.scrollY;
      const docHeight =
        document.documentElement.scrollHeight - window.innerHeight;
      const progress = Math.max(
        0,
        Math.min(1, scrollTop / Math.max(docHeight, 1))
      );
      setScrollProgress(progress);
    };

    window.addEventListener("scroll", handleScroll, { passive: true });
    handleScroll();

    return () => window.removeEventListener("scroll", handleScroll);
  }, [animateOnScroll, fixedProgress]);

  // Update progress when fixedProgress prop changes
  useEffect(() => {
    if (!animateOnScroll) {
      setScrollProgress(fixedProgress);
    }
  }, [fixedProgress, animateOnScroll]);

  const progress = easeInOutCubic(scrollProgress);

  // Calculate layout positions for the 3-container end state
  const totalWidth = CONTAINER_WIDTH * 2 + CENTER_BOX_SIZE + CONTAINER_GAP * 2;
  const leftContainerCenterX = -totalWidth / 2 + CONTAINER_WIDTH / 2;
  const rightContainerCenterX = totalWidth / 2 - CONTAINER_WIDTH / 2;

  // Calculate viewBox based on scale and variant
  // For corner variant, shift viewBox so the base of the structure aligns to bottom-right
  // ViewBox needs to accommodate both the stack (tall) and the 3-container layout (wide)
  const baseViewBox =
    variant === "corner"
      ? { x: -300, y: -420, w: 600, h: 820 } // Wider to fit 3-container layout
      : { x: -350, y: -420, w: 700, h: 820 }; // Wider default for 3-container
  const scaledViewBox = {
    x: baseViewBox.x / scale,
    y: baseViewBox.y / scale,
    w: baseViewBox.w / scale,
    h: baseViewBox.h / scale,
  };
  const viewBox = `${scaledViewBox.x} ${scaledViewBox.y} ${scaledViewBox.w} ${scaledViewBox.h}`;

  return (
    <div
      ref={containerRef}
      className={`overflow-hidden pointer-events-none ${styles.containerClass} ${className}`}
      style={{ zIndex: 0 }}
    >
      <svg
        className={styles.svgClass}
        preserveAspectRatio={
          variant === "corner" ? "xMaxYMax meet" : "xMidYMid meet"
        }
        viewBox={viewBox}
        style={{ ...styles.svgStyle, opacity: finalOpacity }}
      >
        {/* Bracket at top - fades out as boxes transform */}
        {finalShowBracket && (
          <path
            d={`M -80 -400 
                Q -80 -415 -50 -415
                L 50 -415
                Q 80 -415 80 -400`}
            fill="none"
            stroke="currentColor"
            strokeWidth={3 / scale}
            className="text-neutral-300"
            style={{
              opacity: 1 - progress,
            }}
          />
        )}

        {boxes.map((box) => {
          // Interpolate from stack position to end position
          const currentX = box.stackX + (box.endX - box.stackX) * progress;
          const currentY = box.stackY + (box.endY - box.stackY) * progress;

          // Interpolate size from original to target
          const currentWidth =
            box.width + (box.endWidth - box.width) * progress;
          const currentHeight =
            box.height + (box.endHeight - box.height) * progress;

          // Nebraska block (id 5) gets the alert/error color
          const isNebraskaBlock = box.id === 5;
          const strokeColor = isNebraskaBlock ? "#e52e61" : "currentColor";
          const fillColor = isNebraskaBlock ? "#e52e61" : "none";

          return (
            <rect
              key={box.id}
              x={-currentWidth / 2}
              y={-currentHeight / 2}
              width={currentWidth}
              height={currentHeight}
              fill={fillColor}
              stroke={strokeColor}
              strokeWidth={(isNebraskaBlock ? 1.5 : 1) / scale}
              className={isNebraskaBlock ? "" : "text-neutral-300"}
              rx={(progress * 6) / scale}
              ry={(progress * 6) / scale}
              style={{
                transform: `translate(${currentX}px, ${currentY}px)`,
                transformOrigin: "center",
              }}
            />
          );
        })}

        {/* Center "iii" logo box - fades in as progress increases */}
        <g style={{ opacity: progress }}>
          {/* Center box border */}
          <rect
            x={-CENTER_BOX_SIZE / 2}
            y={-CENTER_BOX_SIZE / 2}
            width={CENTER_BOX_SIZE}
            height={CENTER_BOX_SIZE}
            fill="none"
            stroke="currentColor"
            strokeWidth={1.5 / scale}
            className="text-neutral-300"
            rx={6 / scale}
            ry={6 / scale}
          />
          {/* iii logo inside center box */}
          <g transform={`translate(0, 0) scale(${2.2 / scale})`}>
            {/* First i */}
            <rect
              className="fill-current text-neutral-400"
              x={-10}
              y={-9}
              width={4}
              height={4}
            />
            <rect
              className="fill-current text-neutral-400"
              x={-10}
              y={-3}
              width={4}
              height={12}
            />
            {/* Second i */}
            <rect
              className="fill-current text-neutral-400"
              x={-2}
              y={-9}
              width={4}
              height={4}
            />
            <rect
              className="fill-current text-neutral-400"
              x={-2}
              y={-3}
              width={4}
              height={12}
            />
            {/* Third i */}
            <rect
              className="fill-current text-neutral-400"
              x={6}
              y={-9}
              width={4}
              height={4}
            />
            <rect
              className="fill-current text-neutral-400"
              x={6}
              y={-3}
              width={4}
              height={12}
            />
          </g>
        </g>

        {/* Left container outline - fades in as progress increases */}
        <rect
          x={leftContainerCenterX - CONTAINER_WIDTH / 2}
          y={-CONTAINER_HEIGHT / 2}
          width={CONTAINER_WIDTH}
          height={CONTAINER_HEIGHT}
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5 / scale}
          className="text-neutral-300"
          rx={8 / scale}
          ry={8 / scale}
          style={{ opacity: progress * 0.5 }}
        />

        {/* Right container outline - fades in as progress increases */}
        <rect
          x={rightContainerCenterX - CONTAINER_WIDTH / 2}
          y={-CONTAINER_HEIGHT / 2}
          width={CONTAINER_WIDTH}
          height={CONTAINER_HEIGHT}
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5 / scale}
          className="text-neutral-300"
          rx={8 / scale}
          ry={8 / scale}
          style={{ opacity: progress * 0.5 }}
        />

        {/* Animated dashed connection lines */}
        <defs>
          {/* Info color (outward from iii) */}
          <linearGradient id="gradientInfo" x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="#42e7e7" stopOpacity="0.2" />
            <stop offset="40%" stopColor="#42e7e7" stopOpacity="1" />
            <stop offset="60%" stopColor="#42e7e7" stopOpacity="1" />
            <stop offset="100%" stopColor="#42e7e7" stopOpacity="0.2" />
          </linearGradient>
          {/* Success color (inward to iii) */}
          <linearGradient
            id="gradientSuccess"
            x1="0%"
            y1="0%"
            x2="100%"
            y2="0%"
          >
            <stop offset="0%" stopColor="#1ce669" stopOpacity="0.2" />
            <stop offset="40%" stopColor="#1ce669" stopOpacity="1" />
            <stop offset="60%" stopColor="#1ce669" stopOpacity="1" />
            <stop offset="100%" stopColor="#1ce669" stopOpacity="0.2" />
          </linearGradient>
        </defs>

        {/* 11 Animated connection lines */}
        <g style={{ opacity: progress }}>
          {/* === LEFT SIDE LINES (6 lines) === */}

          {/* Line 1: From left container edge, top area - INWARD (success/green) */}
          <line
            x1={leftContainerCenterX + CONTAINER_WIDTH / 2}
            y1={-45}
            x2={-CENTER_BOX_SIZE / 2}
            y2={-25}
            stroke="url(#gradientSuccess)"
            strokeWidth={2 / scale}
            strokeDasharray={`${10 / scale} ${20 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 1.8s linear infinite" }}
          />

          {/* Line 2: From a left box (box 2) - OUTWARD (info/cyan) */}
          <line
            x1={boxes[2]?.endX ?? leftContainerCenterX + 40}
            y1={boxes[2]?.endY ?? -30}
            x2={-CENTER_BOX_SIZE / 2}
            y2={-8}
            stroke="url(#gradientInfo)"
            strokeWidth={2 / scale}
            strokeDasharray={`${8 / scale} ${25 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowLeft 2.2s linear infinite" }}
          />

          {/* Line 3: From left container center - INWARD (success/green) */}
          <line
            x1={leftContainerCenterX + CONTAINER_WIDTH / 2}
            y1={0}
            x2={-CENTER_BOX_SIZE / 2}
            y2={0}
            stroke="url(#gradientSuccess)"
            strokeWidth={2.5 / scale}
            strokeDasharray={`${12 / scale} ${18 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 2.5s linear infinite" }}
          />

          {/* Line 4: From a left box (box 5) - OUTWARD (info/cyan) */}
          <line
            x1={boxes[5]?.endX ?? leftContainerCenterX + 30}
            y1={boxes[5]?.endY ?? 20}
            x2={-CENTER_BOX_SIZE / 2}
            y2={10}
            stroke="url(#gradientInfo)"
            strokeWidth={1.5 / scale}
            strokeDasharray={`${6 / scale} ${22 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowLeft 1.6s linear infinite" }}
          />

          {/* Line 5: From left container edge, bottom area - INWARD (success/green) */}
          <line
            x1={leftContainerCenterX + CONTAINER_WIDTH / 2}
            y1={40}
            x2={-CENTER_BOX_SIZE / 2}
            y2={22}
            stroke="url(#gradientSuccess)"
            strokeWidth={2 / scale}
            strokeDasharray={`${9 / scale} ${24 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 2.1s linear infinite" }}
          />

          {/* Line 6: From a left box (box 7) - INWARD (success/green) - switches direction */}
          <line
            x1={boxes[7]?.endX ?? leftContainerCenterX + 50}
            y1={boxes[7]?.endY ?? 50}
            x2={-CENTER_BOX_SIZE / 2}
            y2={35}
            stroke="url(#gradientSuccess)"
            strokeWidth={1.5 / scale}
            strokeDasharray={`${7 / scale} ${28 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 2.8s linear infinite" }}
          />

          {/* === RIGHT SIDE LINES (5 lines) === */}

          {/* Line 7: To right container edge, top area - OUTWARD (info/cyan) */}
          <line
            x1={CENTER_BOX_SIZE / 2}
            y1={-28}
            x2={rightContainerCenterX - CONTAINER_WIDTH / 2}
            y2={-50}
            stroke="url(#gradientInfo)"
            strokeWidth={2 / scale}
            strokeDasharray={`${10 / scale} ${20 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 1.9s linear infinite" }}
          />

          {/* Line 8: To a right box (box 12) - INWARD (success/green) */}
          <line
            x1={CENTER_BOX_SIZE / 2}
            y1={-10}
            x2={boxes[12]?.endX ?? rightContainerCenterX - 35}
            y2={boxes[12]?.endY ?? -25}
            stroke="url(#gradientSuccess)"
            strokeWidth={1.5 / scale}
            strokeDasharray={`${8 / scale} ${22 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowLeft 2.4s linear infinite" }}
          />

          {/* Line 9: To right container center - OUTWARD (info/cyan) */}
          <line
            x1={CENTER_BOX_SIZE / 2}
            y1={5}
            x2={rightContainerCenterX - CONTAINER_WIDTH / 2}
            y2={0}
            stroke="url(#gradientInfo)"
            strokeWidth={2.5 / scale}
            strokeDasharray={`${12 / scale} ${18 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 2.0s linear infinite" }}
          />

          {/* Line 10: To a right box (box 18) - OUTWARD (info/cyan) */}
          <line
            x1={CENTER_BOX_SIZE / 2}
            y1={18}
            x2={boxes[18]?.endX ?? rightContainerCenterX - 20}
            y2={boxes[18]?.endY ?? 30}
            stroke="url(#gradientInfo)"
            strokeWidth={1.5 / scale}
            strokeDasharray={`${6 / scale} ${26 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowRight 1.7s linear infinite" }}
          />

          {/* Line 11: To right container edge, bottom - INWARD (success/green) - switches direction */}
          <line
            x1={CENTER_BOX_SIZE / 2}
            y1={32}
            x2={rightContainerCenterX - CONTAINER_WIDTH / 2}
            y2={45}
            stroke="url(#gradientSuccess)"
            strokeWidth={2 / scale}
            strokeDasharray={`${9 / scale} ${21 / scale}`}
            strokeLinecap="round"
            style={{ animation: "dashFlowLeft 2.6s linear infinite" }}
          />
        </g>

        {/* CSS animations for dash flow */}
        <style>
          {`
            @keyframes dashFlowRight {
              0% { stroke-dashoffset: ${80 / scale}; }
              100% { stroke-dashoffset: ${-80 / scale}; }
            }
            @keyframes dashFlowLeft {
              0% { stroke-dashoffset: ${-80 / scale}; }
              100% { stroke-dashoffset: ${80 / scale}; }
            }
          `}
        </style>
      </svg>
    </div>
  );
}

// Pre-configured variants for easy use

/** Full-screen background that animates on scroll */
export function DependencyStackBackground(
  props: Omit<DependencyStackProps, "variant">
) {
  return <DependencyStack variant="fullscreen" {...props} />;
}

/** Small corner decoration in bottom-left */
export function DependencyStackCorner(
  props: Omit<DependencyStackProps, "variant">
) {
  return <DependencyStack variant="corner" scale={0.6} {...props} />;
}

/** Centered splash/hero element */
export function DependencyStackSplash(
  props: Omit<DependencyStackProps, "variant">
) {
  return <DependencyStack variant="splash" {...props} />;
}
