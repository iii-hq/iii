/**
 * Types for the Streams plugin
 */

export interface StreamInfo {
  id: string
  name: string
  hidden: boolean
  itemCount: number
  groupCount: number
  lastUpdated?: number
  filePath?: string
}

export interface StreamItem {
  id: string
  groupId: string
  streamName: string
  data: unknown
  updatedAt?: number
}

export type StreamViewMode = 'list' | 'json' | 'table'
