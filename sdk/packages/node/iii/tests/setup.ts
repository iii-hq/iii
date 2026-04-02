import { afterAll } from 'vitest'
import { bridgeIII } from './bridge-utils'
import { iii } from './utils'

afterAll(async () => {
  const sdks = [iii, bridgeIII] as { shutdown?: () => Promise<void> }[]

  for (const sdk of sdks) {
    try {
      if (sdk.shutdown) {
        await sdk.shutdown()
      }
    } catch (error) {
      console.error('Error shutting down SDK:', error)
    }
  }
})
