import { cn } from '@motiadev/ui'
import { Database, FileCode, Folder, Key, Server, Unlock } from 'lucide-react'
import { memo, useCallback, useEffect, useRef, useState } from 'react'
import { useStreamDetails } from '../hooks/use-stream-details'
import { useStreams } from '../hooks/use-streams'
import { useStreamsStore } from '../stores/use-streams-store'
import {
  ActivityIcon,
  ChevronDownIcon,
  type ChevronDownIconHandle,
  ChevronRightIcon,
  LayersIcon,
  LoaderPinwheelIcon,
  LockIcon,
  RefreshCwIcon,
  type RefreshCwIconHandle,
  XIcon,
  ZapIcon,
} from './lucide-animated'
import { StreamItemsTable } from './stream-items-table'
import { StreamSchemaView } from './stream-schema-view'

const COMMON_GROUPS = [
  'default',
  'pending',
  'active',
  'completed',
  'failed',
  'processing',
  'queue',
  'conversations',
  'escalations',
  'refunds',
  'analytics',
]

interface GroupInfo {
  id: string
  count: number
  loading: boolean
}

export const StreamDetail = memo(() => {
  const selectedStreamId = useStreamsStore((state) => state.selectedStreamId)
  const selectedGroupId = useStreamsStore((state) => state.selectedGroupId)
  const selectStream = useStreamsStore((state) => state.selectStream)
  const selectGroup = useStreamsStore((state) => state.selectGroup)

  const { streams } = useStreams()
  const { streamDetails, items, loading, error, refetchDetails } = useStreamDetails()

  const [groups, setGroups] = useState<GroupInfo[]>([])
  const [loadingGroups, setLoadingGroups] = useState(false)
  const [showSchema, setShowSchema] = useState(false)

  const refreshIconRef = useRef<RefreshCwIconHandle>(null)
  const schemaChevronRef = useRef<ChevronDownIconHandle>(null)

  const selectedStream = streams.find((s) => s.id === selectedStreamId)

  const discoverGroups = useCallback(
    async (streamName: string) => {
      setLoadingGroups(true)
      const discoveredGroups: GroupInfo[] = []

      for (const groupId of COMMON_GROUPS) {
        try {
          const response = await fetch(
            `/__motia/streams/group/${encodeURIComponent(streamName)}/${encodeURIComponent(groupId)}`,
          )
          if (response.ok) {
            const data = await response.json()
            if (data.items && data.items.length > 0) {
              discoveredGroups.push({
                id: groupId,
                count: data.count || data.items.length,
                loading: false,
              })
            }
          }
        } catch {
          // Silently skip groups that don't exist or can't be accessed
        }
      }

      if (!discoveredGroups.find((g) => g.id === 'default')) {
        discoveredGroups.unshift({ id: 'default', count: 0, loading: false })
      }

      setGroups(discoveredGroups)
      setLoadingGroups(false)

      const firstWithItems = discoveredGroups.find((g) => g.count > 0)
      if (firstWithItems) {
        selectGroup(firstWithItems.id)
      } else if (discoveredGroups.length > 0) {
        selectGroup(discoveredGroups[0].id)
      }
    },
    [selectGroup],
  )

  useEffect(() => {
    if (selectedStreamId) {
      discoverGroups(selectedStreamId)
    }
  }, [selectedStreamId, discoverGroups])

  const handleClose = useCallback(() => {
    selectStream(null)
    selectGroup(null)
  }, [selectStream, selectGroup])

  const handleRefresh = useCallback(() => {
    refreshIconRef.current?.startAnimation()
    refetchDetails()
  }, [refetchDetails])

  const handleSelectGroup = useCallback(
    (groupId: string) => {
      selectGroup(groupId)
    },
    [selectGroup],
  )

  const toggleSchema = useCallback(() => {
    schemaChevronRef.current?.startAnimation()
    setShowSchema(!showSchema)
  }, [showSchema])

  if (!selectedStream) {
    return null
  }

  const schema = streamDetails?.schema as { anyOf?: unknown[]; properties?: Record<string, unknown> } | null
  const schemaTypes = schema?.anyOf?.length || 0
  const hasAccessControl = streamDetails && 'canAccess' in streamDetails

  return (
    <div className="flex flex-col h-full overflow-hidden bg-background">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-card">
        <div className="flex items-center gap-3 min-w-0">
          <div className="p-2 rounded-lg bg-primary/10">
            <Database className="h-5 w-5 text-primary" />
          </div>
          <div className="min-w-0">
            <h2 className="text-base font-semibold text-foreground truncate">{selectedStream.name}</h2>
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <FileCode className="h-3 w-3" />
              <span className="truncate">{selectedStream.filePath?.split('/').slice(-3).join('/')}</span>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleRefresh}
            disabled={loading}
            className={cn(
              'p-2 rounded-md transition-colors',
              'text-muted-foreground hover:text-foreground hover:bg-muted',
            )}
            title="Refresh"
          >
            <RefreshCwIcon ref={refreshIconRef} size={16} />
          </button>
          <button
            type="button"
            onClick={handleClose}
            className="p-2 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
            title="Close"
          >
            <XIcon size={16} />
          </button>
        </div>
      </div>

      <div className="grid grid-cols-4 gap-3 px-4 py-3 border-b border-border bg-card/50">
        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-blue-500/10">
            <Server className="h-4 w-4 text-blue-500" />
          </div>
          <div>
            <div className="text-xs text-muted-foreground">Storage</div>
            <div className="text-sm font-medium text-foreground">Default (Redis)</div>
          </div>
        </div>

        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-purple-500/10">
            <LayersIcon size={16} className="text-purple-500" />
          </div>
          <div>
            <div className="text-xs text-muted-foreground">Schema Types</div>
            <div className="text-sm font-medium text-foreground">
              {schemaTypes > 0 ? `${schemaTypes} types (union)` : 'Single type'}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className={cn('p-2 rounded-md', hasAccessControl ? 'bg-green-500/10' : 'bg-muted')}>
            {hasAccessControl ? (
              <LockIcon size={16} className="text-green-500" />
            ) : (
              <Unlock className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
          <div>
            <div className="text-xs text-muted-foreground">Access Control</div>
            <div className="text-sm font-medium text-foreground">{hasAccessControl ? 'Protected' : 'Public'}</div>
          </div>
        </div>

        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-green-500/10">
            <ActivityIcon size={16} className="text-green-500" />
          </div>
          <div>
            <div className="text-xs text-muted-foreground">Status</div>
            <div className="text-sm font-medium text-green-500">Live</div>
          </div>
        </div>
      </div>

      {schema && (
        <div className="border-b border-border">
          <button
            type="button"
            onClick={toggleSchema}
            className="w-full flex items-center justify-between px-4 py-3 hover:bg-muted/30 transition-colors"
          >
            <div className="flex items-center gap-2">
              <Key className="h-4 w-4 text-muted-foreground" />
              <span className="text-sm font-medium">Data Schema</span>
              <span className="text-xs bg-muted text-muted-foreground px-2 py-0.5 rounded-full">
                {schemaTypes > 0 ? `${schemaTypes} variants` : '1 type'}
              </span>
            </div>
            {showSchema ? (
              <ChevronDownIcon ref={schemaChevronRef} size={16} className="text-muted-foreground" />
            ) : (
              <ChevronRightIcon size={16} className="text-muted-foreground" />
            )}
          </button>
          {showSchema && (
            <div className="px-4 pb-4">
              <StreamSchemaView schema={schema} />
            </div>
          )}
        </div>
      )}

      <div className="px-4 py-3 border-b border-border bg-card/50">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <Folder className="h-4 w-4 text-muted-foreground" />
            <h3 className="text-sm font-medium text-foreground">Groups</h3>
            {loadingGroups && <LoaderPinwheelIcon size={14} className="text-muted-foreground" />}
          </div>
          <div className="text-xs text-muted-foreground">
            {groups.filter((g) => g.count > 0).length} active / {groups.length} total
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          {groups.map((group) => (
            <button
              type="button"
              key={group.id}
              onClick={() => handleSelectGroup(group.id)}
              className={cn(
                'flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-all',
                selectedGroupId === group.id
                  ? 'bg-primary text-primary-foreground shadow-md'
                  : group.count > 0
                    ? 'bg-muted text-foreground hover:bg-muted/80'
                    : 'bg-muted/50 text-muted-foreground hover:bg-muted/80',
              )}
            >
              {group.count > 0 && (
                <ZapIcon
                  size={12}
                  className={cn(selectedGroupId === group.id ? 'text-primary-foreground' : 'text-primary')}
                />
              )}
              <span className="font-medium capitalize">{group.id}</span>
              <span
                className={cn(
                  'text-xs px-1.5 py-0.5 rounded-full min-w-[20px] text-center',
                  selectedGroupId === group.id
                    ? 'bg-primary-foreground/20'
                    : group.count > 0
                      ? 'bg-primary/20 text-primary'
                      : 'bg-background',
                )}
              >
                {group.count}
              </span>
              {group.loading && <LoaderPinwheelIcon size={12} />}
            </button>
          ))}

          {groups.length === 0 && !loadingGroups && (
            <p className="text-sm text-muted-foreground">No groups found. Write data to create groups.</p>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-hidden">
        {error ? (
          <div className="flex items-center justify-center h-full p-8">
            <p className="text-sm text-destructive">{error}</p>
          </div>
        ) : loading ? (
          <div className="flex items-center justify-center h-full p-8">
            <LoaderPinwheelIcon size={32} className="text-muted-foreground" />
          </div>
        ) : selectedGroupId ? (
          <StreamItemsTable items={items} streamName={selectedStreamId || ''} groupId={selectedGroupId} />
        ) : (
          <div className="flex flex-col items-center justify-center h-full p-8 text-center">
            <Folder className="h-12 w-12 text-muted-foreground/50 mb-4" />
            <p className="text-sm text-muted-foreground">Select a group to view items</p>
          </div>
        )}
      </div>
    </div>
  )
})
StreamDetail.displayName = 'StreamDetail'
