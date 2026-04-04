/**
 * Pattern: Realtime Streams
 * Comparable to: Socket.io, Pusher, Firebase Realtime
 *
 * Push live data to connected WebSocket clients.
 * Clients connect at: ws://host:3112/stream/{stream_name}/{group_id}
 *
 * Built-in stream operations: stream::set, stream::get, stream::list,
 * stream::delete, stream::send. Use createStream for custom adapters.
 *
 * How-to references:
 *   - Realtime streams: https://iii.dev/docs/how-to/stream-realtime-data
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'realtime-streams',
})

// ---------------------------------------------------------------------------
// stream::set — Persist an item in a stream group
// Payload: { stream_name, group_id, item_id, data }
// ---------------------------------------------------------------------------
iii.registerFunction('chat::post-message', async (data) => {
  const logger = new Logger()
  const messageId = `msg-${Date.now()}`

  await iii.trigger({
    function_id: 'stream::set',
    payload: {
      stream_name: 'chat',
      group_id: data.room,
      item_id: messageId,
      data: {
        sender: data.sender,
        text: data.text,
        timestamp: new Date().toISOString(),
      },
    },
  })

  logger.info('Message stored in stream', { room: data.room, messageId })
  return { messageId }
})

// ---------------------------------------------------------------------------
// stream::get — Retrieve a single item from a stream group
// Payload: { stream_name, group_id, item_id }
// ---------------------------------------------------------------------------
iii.registerFunction('chat::get-message', async (data) => {
  const message = await iii.trigger({
    function_id: 'stream::get',
    payload: {
      stream_name: 'chat',
      group_id: data.room,
      item_id: data.messageId,
    },
  })

  if (!message) {
    return { error: 'Message not found' }
  }

  return message
})

// ---------------------------------------------------------------------------
// stream::list — List all items in a stream group
// Payload: { stream_name, group_id }
// ---------------------------------------------------------------------------
iii.registerFunction('chat::list-messages', async (data) => {
  const messages = await iii.trigger({
    function_id: 'stream::list',
    payload: {
      stream_name: 'chat',
      group_id: data.room,
    },
  })

  return { room: data.room, messages: messages || [] }
})

// ---------------------------------------------------------------------------
// stream::delete — Remove an item from a stream group
// Payload: { stream_name, group_id, item_id }
// ---------------------------------------------------------------------------
iii.registerFunction('chat::delete-message', async (data) => {
  await iii.trigger({
    function_id: 'stream::delete',
    payload: {
      stream_name: 'chat',
      group_id: data.room,
      item_id: data.messageId,
    },
  })

  return { deleted: data.messageId }
})

// ---------------------------------------------------------------------------
// stream::send — Push a live event to all connected clients
// Clients on ws://host:3112/stream/chat/{room} receive this instantly.
// Use TriggerAction.Void() for fire-and-forget delivery.
// ---------------------------------------------------------------------------
iii.registerFunction('chat::broadcast', async (data) => {
  const logger = new Logger()
  const eventId = `evt-${Date.now()}`

  // Store the message
  await iii.trigger({
    function_id: 'stream::set',
    payload: {
      stream_name: 'chat',
      group_id: data.room,
      item_id: eventId,
      data: {
        sender: data.sender,
        text: data.text,
        timestamp: new Date().toISOString(),
      },
    },
  })

  // Push live event to connected WebSocket clients (fire-and-forget)
  iii.trigger({
    function_id: 'stream::send',
    payload: {
      stream_name: 'chat',
      group_id: data.room,
      id: eventId,
      event_type: 'new_message',
      data: {
        sender: data.sender,
        text: data.text,
        timestamp: new Date().toISOString(),
      },
    },
    action: TriggerAction.Void(),
  })

  logger.info('Message broadcast', { room: data.room, eventId })
  return { eventId }
})

// ---------------------------------------------------------------------------
// createStream — Custom stream adapter with get/set/delete/list/listGroups
// Useful for integrating external data sources as stream backends.
// ---------------------------------------------------------------------------
iii.createStream('presence', {
  get: async ({ group_id, item_id }) => {
    return await iii.trigger({
      function_id: 'state::get',
      payload: { scope: `presence::${group_id}`, key: item_id },
    })
  },
  set: async ({ group_id, item_id, data }) => {
    await iii.trigger({
      function_id: 'state::set',
      payload: {
        scope: `presence::${group_id}`,
        key: item_id,
        value: { ...data, updated_at: new Date().toISOString() },
      },
    })
  },
  delete: async ({ group_id, item_id }) => {
    await iii.trigger({
      function_id: 'state::delete',
      payload: { scope: `presence::${group_id}`, key: item_id },
    })
  },
  list: async ({ group_id }) => {
    return await iii.trigger({
      function_id: 'state::list',
      payload: { scope: `presence::${group_id}` },
    })
  },
  listGroups: async () => {
    const registry = await iii.trigger({
      function_id: 'state::get',
      payload: { scope: 'presence-registry', key: 'groups' },
    })
    return registry?.groups || []
  },
})

// ---------------------------------------------------------------------------
// Presence tracking — user joins/leaves
// Clients connect at: ws://host:3112/stream/presence/{room}
// ---------------------------------------------------------------------------
iii.registerFunction('presence::join', async (data) => {
  await iii.trigger({
    function_id: 'stream::set',
    payload: {
      stream_name: 'presence',
      group_id: data.room,
      item_id: data.userId,
      data: { userId: data.userId, name: data.name, status: 'online' },
    },
  })

  // Notify all connected clients
  iii.trigger({
    function_id: 'stream::send',
    payload: {
      stream_name: 'presence',
      group_id: data.room,
      id: `join-${Date.now()}`,
      event_type: 'user_joined',
      data: { userId: data.userId, name: data.name },
    },
    action: TriggerAction.Void(),
  })

  return { joined: data.room }
})

iii.registerFunction('presence::leave', async (data) => {
  await iii.trigger({
    function_id: 'stream::delete',
    payload: {
      stream_name: 'presence',
      group_id: data.room,
      item_id: data.userId,
    },
  })

  iii.trigger({
    function_id: 'stream::send',
    payload: {
      stream_name: 'presence',
      group_id: data.room,
      id: `leave-${Date.now()}`,
      event_type: 'user_left',
      data: { userId: data.userId },
    },
    action: TriggerAction.Void(),
  })

  return { left: data.room }
})

// ---------------------------------------------------------------------------
// HTTP triggers
// ---------------------------------------------------------------------------
iii.registerTrigger({ type: 'http', function_id: 'chat::broadcast', config: { api_path: '/chat/send', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'chat::list-messages', config: { api_path: '/chat/:room/messages', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'presence::join', config: { api_path: '/presence/join', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'presence::leave', config: { api_path: '/presence/leave', http_method: 'POST' } })
