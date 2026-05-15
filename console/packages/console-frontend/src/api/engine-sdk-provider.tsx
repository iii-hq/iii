import { type ISdk, registerWorker } from 'iii-browser-sdk'
import { createContext, type ReactNode, useContext, useEffect, useState } from 'react'
import { getEngineBridgeWs } from './config'

const EngineSdkContext = createContext<ISdk | null>(null)

export function useEngineSdk(): ISdk {
  const sdk = useContext(EngineSdkContext)
  if (!sdk) {
    throw new Error('useEngineSdk must be used within an EngineSdkProvider')
  }
  return sdk
}

interface EngineSdkProviderProps {
  children: ReactNode
}

export function EngineSdkProvider({ children }: EngineSdkProviderProps) {
  // ConfigProvider gates render until config is loaded (see config-provider.tsx),
  // so getConfig() inside getEngineBridgeWs() is safe at mount time. useState
  // with a lazy initializer is invoked exactly once per mount even in
  // React 18 StrictMode, so we don't accidentally open two WebSockets in dev.
  const [sdk] = useState(() =>
    registerWorker(getEngineBridgeWs(), {
      invocationTimeoutMs: 30_000,
    }),
  )

  // Close the WebSocket and cancel pending invocations on unmount (HMR teardown
  // or conditional render of the provider). Without this, the SDK reconnects
  // forever in the background.
  useEffect(() => {
    return () => {
      void sdk.shutdown()
    }
  }, [sdk])

  return <EngineSdkContext.Provider value={sdk}>{children}</EngineSdkContext.Provider>
}
