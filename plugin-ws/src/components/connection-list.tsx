import { Badge, Button, cn, Empty, EmptyDescription, EmptyTitle, LevelDot } from '@motiadev/ui'
import { Radio, X } from 'lucide-react'
import type React from 'react'
import { useCallback } from 'react'
import { useWebSocketStore } from '../stores/websocket-store'
import type { WebSocketConnection } from '../types/websocket'

interface ConnectionListProps {
  connections: WebSocketConnection[]
}

const statusVariants: Record<WebSocketConnection['status'], 'warning' | 'success' | 'default' | 'error'> = {
  connecting: 'warning',
  connected: 'success',
  disconnected: 'default',
  error: 'error',
}

export const ConnectionList: React.FC<ConnectionListProps> = ({ connections }) => {
  const { selectedConnectionId, selectConnection, removeConnection } = useWebSocketStore()

  const handleDelete = useCallback(
    async (e: React.MouseEvent, id: string) => {
      e.stopPropagation()
      removeConnection(id)
      if (selectedConnectionId === id) {
        selectConnection(null)
      }
    },
    [removeConnection, selectConnection, selectedConnectionId],
  )

  if (connections.length === 0) {
    return (
      <Empty className="h-32">
        <EmptyTitle className="flex items-center gap-2">
          <Radio className="w-4 h-4 text-muted-foreground" />
          No WebSocket connections
        </EmptyTitle>
        <EmptyDescription>Track connections using the API or create a test connection</EmptyDescription>
      </Empty>
    )
  }

  return (
    <div className="flex flex-col divide-y divide-border/50">
      {connections.map((connection) => (
        <button
          key={connection.id}
          onClick={() => selectConnection(connection.id)}
          className={cn(
            'flex items-start gap-3 px-4 py-3 text-left',
            'hover:bg-accent/50 transition-all duration-200 group relative',
            'focus:outline-none focus:ring-1 focus:ring-primary/50 focus:ring-inset',
            selectedConnectionId === connection.id && 'bg-accent/70 shadow-inner',
          )}
        >
          <div className="mt-1.5 flex-shrink-0">
            <LevelDot level={statusVariants[connection.status]} />
          </div>
          <div className="flex-1 min-w-0">
            <div className="text-sm font-medium text-foreground truncate">{connection.url}</div>
            <div className="flex items-center gap-2 mt-1.5">
              <Badge variant={statusVariants[connection.status]} className="capitalize text-[10px]">
                {connection.status}
              </Badge>
              <span className="text-xs text-muted-foreground">{connection.messageCount} messages</span>
            </div>
            <div className="text-[10px] text-muted-foreground/60 mt-1 truncate font-mono">{connection.id}</div>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={(e) => handleDelete(e, connection.id)}
            className={cn(
              'opacity-0 group-hover:opacity-100 transition-opacity h-7 w-7',
              'hover:bg-destructive/10 hover:text-destructive',
            )}
          >
            <X className="w-3.5 h-3.5" />
          </Button>
        </button>
      ))}
    </div>
  )
}
