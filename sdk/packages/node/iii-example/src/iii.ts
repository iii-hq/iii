import { registerWorker } from 'iii-sdk'
import { version } from '../package.json'

/** @ts-expect-error process.env is not typed */
// Engine WebSocket URL - used for both III and telemetry
const engineWsUrl = process.env.III_URL ?? 'ws://localhost:49134'

export const iii = registerWorker(engineWsUrl, {
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
