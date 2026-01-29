import { Bridge, recordInvocation } from '@iii-dev/sdk'

export const bridge = new Bridge(process.env.III_BRIDGE_URL ?? 'ws://localhost:49134')

export { recordInvocation }

let metricsStarted = false

bridge.on('open', () => {
  console.log('[iii-example] Connected to engine, metrics will start in 3s')
  if (!metricsStarted) {
    metricsStarted = true
    setTimeout(() => {
      console.log('[iii-example] Starting real metrics reporting...')
      bridge.startMetricsReporting(5000)
    }, 3000)
  }
})
