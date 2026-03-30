import { afterAll } from 'vitest'
import { iii } from './utils'

afterAll(async () => {
  try {
    await iii.shutdown()
  } catch {
    // ignore
  }
})
