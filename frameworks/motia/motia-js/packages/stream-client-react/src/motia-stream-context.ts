import type { Stream } from '@motiadev/stream-client-browser'
import React from 'react'

type MotiaStreamContextType = {
  stream: Stream | null
}

export const MotiaStreamContext = React.createContext<MotiaStreamContextType>({
  stream: null,
})
