import { AlertTriangle, Inbox, Send, Settings, Users } from 'lucide-react'
import { useState } from 'react'
import type { QueueStats } from '@/api/queues/queues'
import { Button } from '@/components/ui/card'
import { JsonViewer } from '@/components/ui/json-viewer'

interface QueueOverviewTabProps {
  stats: QueueStats | undefined
  onPublish: (json: string) => void
  isPublishing: boolean
  publishError: string | null
  onClearPublishError: () => void
}

export function QueueOverviewTab({
  stats,
  onPublish,
  isPublishing,
  publishError,
  onClearPublishError,
}: QueueOverviewTabProps) {
  const [showPublish, setShowPublish] = useState(false)
  const [publishJson, setPublishJson] = useState('')

  return (
    <div className="flex-1 overflow-y-auto p-4 space-y-4">
      {/* Stats */}
      <div className="grid grid-cols-3 gap-2">
        <div className="rounded-[var(--radius-lg)] bg-elevated border border-border-subtle p-3">
          <div className="flex items-center gap-1.5 mb-1.5">
            <Inbox className="w-3 h-3 text-muted" />
            <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
              Depth
            </span>
          </div>
          <div className="text-lg font-mono text-foreground">{stats?.depth ?? 0}</div>
        </div>
        <div className="rounded-[var(--radius-lg)] bg-elevated border border-border-subtle p-3">
          <div className="flex items-center gap-1.5 mb-1.5">
            <Users className="w-3 h-3 text-muted" />
            <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
              Consumers
            </span>
          </div>
          <div className="text-lg font-mono text-foreground">{stats?.consumer_count ?? 0}</div>
        </div>
        <div className="rounded-[var(--radius-lg)] bg-elevated border border-border-subtle p-3">
          <div className="flex items-center gap-1.5 mb-1.5">
            <AlertTriangle
              className={`w-3 h-3 ${(stats?.dlq_depth ?? 0) > 0 ? 'text-error' : 'text-muted'}`}
            />
            <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
              DLQ
            </span>
          </div>
          <div
            className={`text-lg font-mono ${(stats?.dlq_depth ?? 0) > 0 ? 'text-error' : 'text-foreground'}`}
          >
            {stats?.dlq_depth ?? 0}
          </div>
        </div>
      </div>

      {/* Config */}
      {stats?.config && (
        <div>
          <div className="flex items-center gap-1.5 mb-1.5">
            <Settings className="w-3 h-3 text-muted" />
            <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
              Configuration
            </span>
          </div>
          <div className="rounded-[var(--radius-lg)] bg-elevated border border-border-subtle p-3 overflow-x-auto max-h-64 overflow-y-auto">
            <JsonViewer data={stats.config} collapsed={true} maxDepth={4} />
          </div>
        </div>
      )}

      {/* Publish */}
      <div className="space-y-3">
        {!showPublish ? (
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowPublish(true)}
            className="w-full gap-2"
          >
            <Send className="w-3.5 h-3.5" />
            Publish Message
          </Button>
        ) : (
          <div className="space-y-3">
            <div>
              <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-1.5">
                Payload
              </div>
              <div className="text-[10px] text-secondary mb-1">
                Type the JSON your subscriber will receive. The console wraps it in{' '}
                <code className="text-muted">{'{ data: ... }'}</code> automatically.
              </div>
              <textarea
                value={publishJson}
                onChange={(e) => {
                  setPublishJson(e.target.value)
                  onClearPublishError()
                }}
                className="w-full h-32 text-xs font-mono bg-black/40 text-foreground px-3 py-2 rounded border border-border focus:border-accent resize-none transition-colors"
                style={{ outline: 'none' }}
                placeholder='{"key": "value"}'
              />
            </div>
            {publishError && (
              <div className="flex items-center gap-2 text-[11px] text-error bg-error/10 border border-error/20 rounded px-3 py-2">
                <AlertTriangle className="w-3 h-3 shrink-0" />
                {publishError}
              </div>
            )}
            <div className="flex gap-2">
              <Button
                variant="accent"
                size="sm"
                onClick={() => onPublish(publishJson)}
                disabled={isPublishing}
                className="flex-1"
              >
                {isPublishing ? 'Publishing...' : 'Publish'}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setShowPublish(false)
                  onClearPublishError()
                }}
              >
                Cancel
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
