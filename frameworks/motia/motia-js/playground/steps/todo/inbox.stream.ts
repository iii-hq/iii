import { Stream, type StreamConfig } from 'motia'
import { z } from 'zod'

const inbox = z.object({
  watching: z.number(),
})

export const config: StreamConfig = {
  baseConfig: { storageType: 'default' },
  name: 'inbox',
  schema: inbox,
}

export const inboxStream = new Stream(config)
export type Inbox = z.infer<typeof inbox>
