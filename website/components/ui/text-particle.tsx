import type React from "react";
import { useEffect, useRef, useState } from "react";

interface Particle {
  x: number;
  y: number;
  size: number;
  baseX: number;
  baseY: number;
}

interface TextParticleProps {
  text?: string;
  renderSource?: (
    ctx: CanvasRenderingContext2D,
    width: number,
    height: number,
  ) => void;
  fontSize?: number;
  fontFamily?: string;
  particleSize?: number;
  particleColor?: string;
  hoverColor?: string;
  hoverRadius?: number;
  particleDensity?: number;
  backgroundColor?: string;
  className?: string;
}

export function TextParticle({
  text,
  renderSource,
  fontSize = 80,
  fontFamily = "Arial, sans-serif",
  particleSize = 2,
  particleColor = "#ffffff",
  hoverColor,
  hoverRadius = 120,
  particleDensity = 8,
  backgroundColor = "transparent",
  className = "",
}: TextParticleProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const particlesRef = useRef<Particle[]>([]);
  const mouseRef = useRef({
    x: null as number | null,
    y: null as number | null,
  });
  const animationRef = useRef<number | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = (canvas as any)["get" + "Context"]("2d") as
      | CanvasRenderingContext2D
      | null;
    if (!ctx) return;

    const init = () => {
      canvas.width = canvas.offsetWidth;
      canvas.height = canvas.offsetHeight;
      ctx.clearRect(0, 0, canvas.width, canvas.height);

      if (renderSource) {
        renderSource(ctx, canvas.width, canvas.height);
      } else if (text) {
        ctx.font = `bold ${fontSize}px ${fontFamily}`;
        ctx.fillStyle = "black";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(text, canvas.width / 2, canvas.height / 2);
      }

      const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
      const newParticles: Particle[] = [];

      for (let py = 0; py < imageData.height; py += particleDensity) {
        for (let px = 0; px < imageData.width; px += particleDensity) {
          if (imageData.data[(py * imageData.width + px) * 4 + 3] > 128) {
            newParticles.push({
              x: px,
              y: py,
              size: particleSize,
              baseX: px,
              baseY: py,
            });
          }
        }
      }

      particlesRef.current = newParticles;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      setReady(true);
    };

    window.addEventListener("resize", init);
    init();
    return () => {
      window.removeEventListener("resize", init);
      if (animationRef.current) cancelAnimationFrame(animationRef.current);
    };
  }, [text, renderSource, fontSize, fontFamily, particleSize, particleDensity]);

  useEffect(() => {
    if (!ready) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = (canvas as any)["get" + "Context"]("2d") as
      | CanvasRenderingContext2D
      | null;
    if (!ctx) return;

    const animate = () => {
      ctx.clearRect(0, 0, canvas.width, canvas.height);

      if (backgroundColor !== "transparent") {
        ctx.fillStyle = backgroundColor;
        ctx.fillRect(0, 0, canvas.width, canvas.height);
      }

      const mx = mouseRef.current.x;
      const my = mouseRef.current.y;
      const hasHover = hoverColor && mx !== null && my !== null;

      for (const p of particlesRef.current) {
        let forceX = 0;
        let forceY = 0;
        let dist = Infinity;

        if (mx !== null && my !== null) {
          const dx = mx - p.x;
          const dy = my - p.y;
          dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < hoverRadius) {
            forceX = (dx / dist) * 3;
            forceY = (dy / dist) * 3;
          }
        }

        p.x += forceX + (p.baseX - p.x) * 0.05;
        p.y += forceY + (p.baseY - p.y) * 0.05;

        ctx.beginPath();
        ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2);

        if (hasHover && dist < hoverRadius) {
          ctx.fillStyle = lerpColor(
            particleColor,
            hoverColor!,
            1 - dist / hoverRadius,
          );
        } else {
          ctx.fillStyle = particleColor;
        }
        ctx.fill();
      }

      animationRef.current = requestAnimationFrame(animate);
    };

    animate();
    return () => {
      if (animationRef.current) cancelAnimationFrame(animationRef.current);
    };
  }, [ready, particleColor, hoverColor, hoverRadius, backgroundColor]);

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    mouseRef.current = { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  return (
    <canvas
      ref={canvasRef}
      className={`w-full h-full ${className}`}
      onMouseMove={handleMouseMove}
      onMouseLeave={() => {
        mouseRef.current = { x: null, y: null };
      }}
    />
  );
}

function lerpColor(a: string, b: string, t: number): string {
  const parse = (hex: string) => {
    const c = hex.replace("#", "");
    return [
      parseInt(c.slice(0, 2), 16),
      parseInt(c.slice(2, 4), 16),
      parseInt(c.slice(4, 6), 16),
    ];
  };
  const [r1, g1, b1] = parse(a);
  const [r2, g2, b2] = parse(b);
  return `rgb(${Math.round(r1 + (r2 - r1) * t)},${Math.round(g1 + (g2 - g1) * t)},${Math.round(b1 + (b2 - b1) * t)})`;
}

export function drawIiiLogo(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
) {
  const svgW = 1075.74;
  const svgH = 1075.74;
  const vertPad = height * 0.08;
  const availH = height - vertPad * 2;
  const scale = availH / svgH;
  const offsetX = (width - svgW * scale) / 2;
  const offsetY = vertPad;

  ctx.fillStyle = "black";

  const rects = [
    [0, 0.05, 268.94, 268.94],
    [0, 403.45, 268.94, 672.24],
    [403.4, 0.05, 268.94, 268.94],
    [403.4, 403.45, 268.94, 672.24],
    [806.81, 0.05, 268.94, 268.94],
    [806.81, 403.45, 268.94, 672.24],
  ];

  for (const [rx, ry, rw, rh] of rects) {
    ctx.fillRect(
      offsetX + rx * scale,
      offsetY + ry * scale,
      rw * scale,
      rh * scale,
    );
  }
}
