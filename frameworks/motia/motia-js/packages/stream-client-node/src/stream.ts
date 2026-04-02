import { Stream as StreamClient } from '@motiadev/stream-client'
import { StreamSocketAdapter } from './stream-adapter'

export type NodeStreamOptions = {
  protocols?: string | string[] | undefined
}

export class Stream extends StreamClient {
  constructor(address: string, options?: NodeStreamOptions) {
    super(() => new StreamSocketAdapter(address, options?.protocols))
  }
}
