'use client'

import { useContext } from 'react'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { GraphicPlaybackContext } from './graphic-playback-context'

/**
 * Drives looping SVG animations: `active` is true only while the graphic is on screen
 * and the user has not asked for reduced motion. Graphics render their static end state
 * when `active` is false.
 */
export function useGraphicLoop<T extends Element = SVGSVGElement>() {
  const { ref, inView, running, reduce } = useDemoPlayback<T>()
  const enabled = useContext(GraphicPlaybackContext)
  return { ref, active: running && enabled, inView, reduce }
}
