'use client'

import { useEffect, useRef, useState } from 'react'

import { lastStoryBeat, STORY_REST_MS, type StoryStage, storyBeatDuration } from './story-model'

type Playback = { beat: number; waiting: boolean }

/** First beat → one sequence → completed result held → loops back to the start. Only visible time counts. */
export function useStoryPlayback(stage: StoryStage, running: boolean) {
  const [chapters, setChapters] = useState<Record<StoryStage, Playback>>({
    compose: { beat: 0, waiting: true },
    observe: { beat: 0, waiting: true },
    discover: { beat: 0, waiting: true },
    extend: { beat: 0, waiting: true },
    react: { beat: 0, waiting: true },
  })
  const remaining = useRef<Record<StoryStage, { ms: number }>>({
    compose: { ms: storyBeatDuration('compose', 0) },
    observe: { ms: storyBeatDuration('observe', 0) },
    discover: { ms: storyBeatDuration('discover', 0) },
    extend: { ms: storyBeatDuration('extend', 0) },
    react: { ms: storyBeatDuration('react', 0) },
  })
  const current = chapters[stage]
  const phase = current.waiting ? 'waiting' : current.beat === lastStoryBeat(stage) ? 'holding' : 'playing'

  useEffect(() => {
    if (!running) return
    const budget = remaining.current[stage]
    const startedAt = performance.now()
    let elapsed = false
    const timer = window.setTimeout(() => {
      elapsed = true
      const beat = current.beat === lastStoryBeat(stage) ? 0 : current.beat + 1
      remaining.current[stage] = { ms: beat === lastStoryBeat(stage) ? STORY_REST_MS : storyBeatDuration(stage, beat) }
      setChapters((previous) => ({ ...previous, [stage]: { beat, waiting: false } }))
    }, budget.ms)
    return () => {
      window.clearTimeout(timer)
      if (!elapsed) budget.ms = Math.max(0, budget.ms - (performance.now() - startedAt))
    }
  }, [stage, current, running])

  return { beat: current.beat, phase }
}
