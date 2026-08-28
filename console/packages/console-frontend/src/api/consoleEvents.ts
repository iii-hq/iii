import { getConsoleEventsWs } from './config'

export interface ConsoleEventsHandlers {
  /** A coalesced "these traces changed" tick. Carries ids only, never span
   * data — consumers re-run their own filtered queries (notify-then-query),
   * so the engine stays the single source of filter semantics. */
  onTracesChanged: (traceIds: string[]) => void
  /** Fired on every (re)connect; consumers resync anything missed while
   * disconnected with one refetch. */
  onConnect?: () => void
}

/**
 * Live console-event feed (`/ws/console-events`): the console worker owns a
 * `trace` trigger on the engine and forwards its coalesced ticks to every
 * connected browser. Replaces the old 1s blind polling of the trace list —
 * an idle engine produces no traffic at all.
 */
export function createConsoleEventsSubscription(handlers: ConsoleEventsHandlers): () => void {
  let socket: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let disposed = false

  const connect = () => {
    if (disposed) return
    socket = new WebSocket(getConsoleEventsWs())

    socket.onopen = () => {
      handlers.onConnect?.()
    }

    socket.onmessage = (event) => {
      try {
        const message = JSON.parse(event.data)
        if (message?.type === 'traces_changed') {
          const ids = Array.isArray(message.trace_ids)
            ? message.trace_ids.filter((id: unknown): id is string => typeof id === 'string')
            : []
          handlers.onTracesChanged(ids)
        }
      } catch {
        // Malformed frame — the next tick self-heals.
      }
    }

    socket.onclose = () => {
      socket = null
      if (!disposed) {
        reconnectTimer = setTimeout(connect, 3000)
      }
    }

    socket.onerror = () => {
      socket?.close()
    }
  }

  connect()

  return () => {
    disposed = true
    if (reconnectTimer) clearTimeout(reconnectTimer)
    socket?.close()
  }
}
