import { Bridge } from 'iii'

export const bridge = new Bridge(process.env.III_BRIDGE_URL ?? 'ws://localhost:49134')
