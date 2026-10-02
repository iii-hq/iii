'use client'

import { useState } from 'react'

import { Reveal } from '@/components/site/reveal'
import { AnywhereScene, type DeviceId } from './anywhere-scene'

/** Device scene in a framed panel; the scene highlights whichever device is hovered. */
export function AnywhereExplorer() {
  const [hovered, setHovered] = useState<DeviceId | null>(null)
  return (
    <Reveal delay={0.1} className="graphic-stage mt-12 md:mt-14">
      <AnywhereScene hovered={hovered} onHover={setHovered} />
    </Reveal>
  )
}
