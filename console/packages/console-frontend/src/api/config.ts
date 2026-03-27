export interface ConsoleConfig {
  engineHost: string
  enginePort: number
  wsPort: number
  consolePort: number
  version: string
  enableFlow?: boolean
}

let _config: ConsoleConfig | null = null

export function setConfig(config: ConsoleConfig): void {
  _config = config
}

export function getConfig(): ConsoleConfig {
  if (!_config) {
    throw new Error(
      'Config not initialized. Ensure ConfigProvider has loaded before accessing config.',
    )
  }
  return _config
}

export function getDevtoolsApi(): string {
  return '/api/engine/_console'
}

export function getManagementApi(): string {
  return getDevtoolsApi()
}

export function getStreamsWs(): string {
  const wsProtocol =
    typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const c = getConfig()
  const host = typeof window !== 'undefined' ? window.location.host : `localhost:${c.consolePort}`
  return `${wsProtocol}//${host}/ws/streams`
}

export function getEngineBaseUrl(): string {
  return '/api/engine'
}
