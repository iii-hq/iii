import { Bridge } from '@iii-dev/sdk'

export const bridge = new Bridge(process.env.III_BRIDGE_URL ?? 'ws://localhost:49134')
