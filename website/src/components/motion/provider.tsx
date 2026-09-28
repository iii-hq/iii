"use client"

import { LazyMotion } from "motion/react"
import type { ReactNode } from "react"

const loadFeatures = () => import("./features").then((mod) => mod.default)

/**
 * Every animated element on the site is an `m.*` component, so the animation
 * runtime is not in the initial bundle: it streams in through this provider.
 * `strict` throws on a stray `m.*` import, keeping that guarantee honest.
 */
export function MotionProvider({ children }: { children: ReactNode }) {
  return (
    <LazyMotion features={loadFeatures} strict>
      {children}
    </LazyMotion>
  )
}
