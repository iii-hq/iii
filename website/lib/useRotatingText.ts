import { useState, useEffect } from "react";

interface UseRotatingTextOptions {
  items: string[];
  intervalMs?: number;
  animationDurationMs?: number;
}

interface UseRotatingTextReturn {
  currentIndex: number;
  isAnimating: boolean;
  currentItem: string;
}

/**
 * Hook for rotating through a list of text items with animation.
 * Used for hero headlines and other rotating text effects.
 */
export function useRotatingText({
  items,
  intervalMs = 3000,
  animationDurationMs = 400,
}: UseRotatingTextOptions): UseRotatingTextReturn {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [isAnimating, setIsAnimating] = useState(false);

  useEffect(() => {
    const interval = setInterval(() => {
      setIsAnimating(true);
      setTimeout(() => {
        setCurrentIndex((prev) => (prev + 1) % items.length);
        setIsAnimating(false);
      }, animationDurationMs);
    }, intervalMs);

    return () => clearInterval(interval);
  }, [items, intervalMs, animationDurationMs]);

  return {
    currentIndex,
    isAnimating,
    currentItem: items[currentIndex],
  };
}
