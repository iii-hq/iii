import { Bridge } from '@iii-dev/sdk'
import { version } from '../package.json'

// Engine WebSocket URL - used for both Bridge and telemetry
const engineWsUrl = process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'

export const bridge = new Bridge(engineWsUrl, {
  otel: {
    enabled: true,
    serviceName: 'iii-example',
    metricsEnabled: true,
    serviceVersion: version,
    reconnectionConfig: {
      maxRetries: 10,
    },
    metricsExportIntervalMs: 10000,
  },
})
