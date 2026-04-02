import { Stream } from '@motiadev/stream-client-browser'
import { useEffect, useMemo, useState } from 'react'
import { MotiaStreamContext } from './motia-stream-context'

type Props = React.PropsWithChildren<{
  /**
   * The address of the stream server.
   *
   * @example
   * ```tsx
   * <MotiaStreamProvider address="ws://localhost:3000">
   *   <App />
   * </MotiaStreamProvider>
   * */
  address: string
  protocols?: string | string[] | undefined
}>

export const MotiaStreamProvider: React.FC<Props> = ({ children, address, protocols }) => {
  const [stream, setStream] = useState<Stream | null>(null)

  useEffect(() => {
    const streamInstance = new Stream(address, { protocols })
    setStream(streamInstance)
    return () => streamInstance.close()
  }, [address, protocols])

  const contextValue = useMemo(() => ({ stream }), [stream])

  return <MotiaStreamContext.Provider value={contextValue}>{children}</MotiaStreamContext.Provider>
}
