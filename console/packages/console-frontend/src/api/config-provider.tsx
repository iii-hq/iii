import { useQuery } from '@tanstack/react-query'
import { createContext, useContext } from 'react'
import type { ConsoleConfig } from './config'
import { setConfig } from './config'
import { getConnectionErrorMessage } from './utils'

const ConfigContext = createContext<ConsoleConfig | null>(null)

export function useConfig(): ConsoleConfig {
  const config = useContext(ConfigContext)
  if (!config) {
    throw new Error('useConfig must be used within a ConfigProvider')
  }
  return config
}

interface ConfigProviderProps {
  children: React.ReactNode
}

async function fetchConsoleConfig(): Promise<ConsoleConfig> {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), 5000)

  try {
    const res = await fetch('/api/config', {
      signal: controller.signal,
    })

    if (!res.ok) {
      throw new Error(`Config fetch failed: ${res.status}`)
    }

    const data: ConsoleConfig = await res.json()
    setConfig(data)
    return data
  } catch (error) {
    throw new Error(getConnectionErrorMessage(error, 'Unable to fetch console configuration'))
  } finally {
    clearTimeout(timeout)
  }
}

export function ConfigProvider({ children }: ConfigProviderProps) {
  const { data: config, error } = useQuery({
    queryKey: ['console-config'],
    queryFn: fetchConsoleConfig,
    retry: true,
    retryDelay: 3000,
  })

  if (error) {
    return (
      <div className="fixed inset-0 bg-[#0A0A0A] flex flex-col items-center justify-center font-mono">
        <div className="text-[#F4F4F4] text-sm mb-2">Unable to connect to console server</div>
        <div className="text-[#9CA3AF] text-xs mb-6">{error.message}</div>
        <button
          type="button"
          onClick={() => window.location.reload()}
          className="px-4 py-2 bg-[#F3F724] text-black text-xs font-medium rounded hover:bg-[#F3F724]/90 transition-colors"
        >
          Retry
        </button>
        <div className="text-[#9CA3AF] text-[10px] mt-4">Retrying automatically...</div>
      </div>
    )
  }

  if (!config) {
    return (
      <div className="fixed inset-0 bg-[#0A0A0A] flex items-center justify-center">
        <div className="w-6 h-6 border-2 border-[#F3F724]/30 border-t-[#F3F724] rounded-full animate-spin" />
      </div>
    )
  }

  return <ConfigContext.Provider value={config}>{children}</ConfigContext.Provider>
}
