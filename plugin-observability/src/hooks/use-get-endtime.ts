import { useEffect, useRef, useState } from 'react'
import type { TraceGroup } from '../types/observability'

export const useGetEndTime = (group: TraceGroup | undefined | null) => {
  const groupEndTime = group?.endTime
  const [endTime, setEndTime] = useState(() => groupEndTime || Date.now())
  const intervalRef = useRef<NodeJS.Timeout | null>(null)

  useEffect(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current)
      intervalRef.current = null
    }

    if (groupEndTime) {
      setEndTime(groupEndTime)
    } else {
      intervalRef.current = setInterval(() => setEndTime(Date.now()), 100)
    }

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [groupEndTime])

  return endTime
}
