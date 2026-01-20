import { useEffect, useState } from 'react'
import type { TraceGroup } from '@/types/observability'

export const useGetEndTime = (group: TraceGroup | undefined | null) => {
  const groupEndTime = group?.endTime
  const [endTime, setEndTime] = useState(groupEndTime || Date.now())

  useEffect(() => {
    if (groupEndTime) {
      setEndTime(groupEndTime)
    } else {
      const interval = setInterval(() => setEndTime(Date.now()), 50)
      return () => clearInterval(interval)
    }
  }, [groupEndTime])

  return endTime
}
