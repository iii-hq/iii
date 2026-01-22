import { Bridge, recordInvocation } from '@iii-dev/sdk'

export const bridge = new Bridge(process.env.III_BRIDGE_URL ?? 'ws://localhost:49134')

// Re-export recordInvocation for use in function handlers
export { recordInvocation }

// Start automatic metrics reporting after connection is established
// Uses real CPU, memory, network, and disk metrics (no simulated values)
setTimeout(() => {
  console.log('[iii-example] Starting real metrics reporting...')
  bridge.startMetricsReporting(5000) // Report every 5 seconds
}, 3000)

// Log when metrics are being collected (for debugging)
bridge.on('open', () => {
  console.log('[iii-example] Connected to engine, metrics will start in 3s')
})
