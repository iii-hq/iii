import { type LockedData, StreamAdapter } from '@motiadev/core'
import type { StreamInfo } from '../types/stream'

type StreamRegistryInfo = StreamInfo & { id: string }

/**
 * Stream adapter that exposes information about all registered streams
 * in the Motia application for visualization in the workbench.
 */
export class StreamsRegistryAdapter extends StreamAdapter<StreamRegistryInfo> {
  constructor(private readonly lockedData: LockedData) {
    super('__motia.streams-registry')
  }

  async get(_groupId: string, id: string): Promise<StreamRegistryInfo | null> {
    const streams = this.lockedData.listStreams()
    const stream = streams.find((s) => s.config.name === id || s.filePath === id)

    if (!stream) {
      return null
    }

    return {
      id: stream.config.name,
      name: stream.config.name,
      hidden: stream.hidden || false,
      itemCount: 0, // Will be populated when stream is queried
      groupCount: 0,
      lastUpdated: Date.now(),
      filePath: stream.filePath,
    }
  }

  async set(_groupId: string, _id: string, data: StreamRegistryInfo): Promise<StreamRegistryInfo> {
    return data
  }

  async delete(_groupId: string, id: string): Promise<StreamRegistryInfo | null> {
    return { id } as StreamRegistryInfo
  }

  async getGroup(_groupId: string): Promise<StreamRegistryInfo[]> {
    const streams = this.lockedData.listStreams()

    return streams.map((stream) => ({
      id: stream.config.name,
      name: stream.config.name,
      hidden: stream.hidden || false,
      itemCount: 0,
      groupCount: 0,
      lastUpdated: Date.now(),
      filePath: stream.filePath,
    }))
  }
}
