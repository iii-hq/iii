import { Stream as StreamClient } from '@motiadev/stream-client'
import { StreamSocketAdapter } from './stream-adapter'

export type BrowserStreamOptions = {
  protocols?: string | string[] | undefined
}

export class Stream extends StreamClient {
  constructor(address: string, options?: BrowserStreamOptions) {
    super(() => new StreamSocketAdapter(address, options?.protocols))
  }
}
