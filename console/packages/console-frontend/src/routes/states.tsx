import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Check,
  Copy,
  Database,
  Edit2,
  Folder,
  Key,
  Plus,
  RefreshCw,
  Save,
  Search,
  Send,
  Trash2,
  X,
} from 'lucide-react'
import { useEffect, useMemo, useReducer, useState } from 'react'
import type { StateItem } from '@/api'
import { deleteStateItem, setStateItem, stateGroupsQuery, stateItemsQuery } from '@/api'
import { Badge, Button, Input } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { JsonViewer } from '@/components/ui/json-viewer'
import { Pagination } from '@/components/ui/pagination'
import { Skeleton } from '@/components/ui/skeleton'

// --- addModal reducer ---
interface AddModalState {
  show: boolean
  newKey: string
  newValue: string
  saving: boolean
}
type AddModalAction =
  | { type: 'OPEN_ADD_MODAL' }
  | { type: 'CLOSE_ADD_MODAL' }
  | { type: 'SET_ADD_FORM'; key: string; value: string }
  | { type: 'START_SAVE' }
  | { type: 'SAVE_DONE' }

const addModalInitial: AddModalState = { show: false, newKey: '', newValue: '', saving: false }

function addModalReducer(state: AddModalState, action: AddModalAction): AddModalState {
  switch (action.type) {
    case 'OPEN_ADD_MODAL':
      return { ...state, show: true }
    case 'CLOSE_ADD_MODAL':
      return { show: false, newKey: '', newValue: '', saving: false }
    case 'SET_ADD_FORM':
      return { ...state, newKey: action.key, newValue: action.value }
    case 'START_SAVE':
      return { ...state, saving: true }
    case 'SAVE_DONE':
      return { ...state, saving: false }
    default:
      return state
  }
}

// --- edit reducer ---
interface EditState {
  editingItem: string | null
  editValue: string
}
type EditAction =
  | { type: 'OPEN_EDIT'; itemKey: string; value: string }
  | { type: 'SET_EDIT_VALUE'; value: string }
  | { type: 'CLOSE_EDIT' }

const editInitial: EditState = { editingItem: null, editValue: '' }

function editReducer(state: EditState, action: EditAction): EditState {
  switch (action.type) {
    case 'OPEN_EDIT':
      return { editingItem: action.itemKey, editValue: action.value }
    case 'SET_EDIT_VALUE':
      return { ...state, editValue: action.value }
    case 'CLOSE_EDIT':
      return { editingItem: null, editValue: '' }
    default:
      return state
  }
}

// --- filter reducer ---
interface FilterState {
  sortField: 'key' | 'type'
  sortDirection: 'asc' | 'desc'
}
type FilterAction = { type: 'SET_SORT'; field: 'key' | 'type' }

const filterInitial: FilterState = { sortField: 'key', sortDirection: 'asc' }

function filterReducer(state: FilterState, action: FilterAction): FilterState {
  switch (action.type) {
    case 'SET_SORT':
      if (state.sortField === action.field) {
        return { ...state, sortDirection: state.sortDirection === 'asc' ? 'desc' : 'asc' }
      }
      return { sortField: action.field, sortDirection: 'asc' }
    default:
      return state
  }
}

// --- pagination reducer ---
interface PaginationState {
  itemsPage: number
  itemsPageSize: number
}
type PaginationAction =
  | { type: 'SET_PAGE'; page: number }
  | { type: 'SET_PAGE_SIZE'; pageSize: number }

const paginationInitial: PaginationState = { itemsPage: 1, itemsPageSize: 50 }

function paginationReducer(state: PaginationState, action: PaginationAction): PaginationState {
  switch (action.type) {
    case 'SET_PAGE':
      return { ...state, itemsPage: action.page }
    case 'SET_PAGE_SIZE':
      return { ...state, itemsPageSize: action.pageSize }
    default:
      return state
  }
}

export const Route = createFileRoute('/states')({
  component: StatesPage,
  loader: ({ context: { queryClient } }) => {
    void queryClient.prefetchQuery(stateGroupsQuery())
  },
})

function StatesPage() {
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null)
  const [selectedItem, setSelectedItem] = useState<StateItem | null>(null)
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const [showMobileSidebar, setShowMobileSidebar] = useState(false)

  const [addModal, dispatchAddModal] = useReducer(addModalReducer, addModalInitial)
  const [editState, dispatchEdit] = useReducer(editReducer, editInitial)
  const [filterState, dispatchFilter] = useReducer(filterReducer, filterInitial)
  const [paginationState, dispatchPagination] = useReducer(paginationReducer, paginationInitial)

  const { show: showAddModal, newKey, newValue, saving } = addModal
  const { editingItem, editValue } = editState
  const { sortField, sortDirection } = filterState
  const { itemsPage, itemsPageSize } = paginationState

  const {
    data: groupsData,
    isLoading: loadingGroups,
    refetch: refetchGroups,
  } = useQuery(stateGroupsQuery())

  const {
    data: itemsData,
    isLoading: loadingItems,
    refetch: refetchItems,
  } = useQuery({
    ...stateItemsQuery(selectedGroupId || ''),
    enabled: !!selectedGroupId,
  })

  const groups = groupsData?.groups || []
  const items = itemsData?.items || []
  const loading = loadingGroups

  const filteredGroups = useMemo(() => {
    return groups.filter((g) => {
      if (searchQuery && !g.id.toLowerCase().includes(searchQuery.toLowerCase())) return false
      return true
    })
  }, [groups, searchQuery])

  useEffect(() => {
    if (groups.length > 0 && !selectedGroupId) {
      const firstWithItems = groups.find((g) => g.count > 0)
      if (firstWithItems) {
        setSelectedGroupId(firstWithItems.id)
      } else {
        setSelectedGroupId(groups[0].id)
      }
    }
  }, [groups, selectedGroupId])

  const handleSelectGroup = (groupId: string) => {
    setSelectedGroupId(groupId)
    setSelectedItem(null)
    dispatchPagination({ type: 'SET_PAGE', page: 1 })
  }

  const handleAddItem = async () => {
    if (!selectedGroupId || !newKey) return

    dispatchAddModal({ type: 'START_SAVE' })
    try {
      let value: unknown = newValue
      try {
        value = JSON.parse(newValue)
      } catch {
        // Keep as string if not valid JSON
      }

      await setStateItem(selectedGroupId, newKey, value)
      dispatchAddModal({ type: 'CLOSE_ADD_MODAL' })
      refetchItems()
    } catch {
      // Handle error
    } finally {
      dispatchAddModal({ type: 'SAVE_DONE' })
    }
  }

  const handleDeleteItem = async (item: StateItem) => {
    if (!selectedGroupId) return

    try {
      await deleteStateItem(selectedGroupId, item.key)
      refetchItems()
      if (selectedItem?.key === item.key) {
        setSelectedItem(null)
      }
    } catch {
      // Handle error
    }
  }

  const handleEditItem = async (item: StateItem) => {
    if (!selectedGroupId) return

    dispatchAddModal({ type: 'START_SAVE' })
    try {
      let value: unknown = editValue
      try {
        value = JSON.parse(editValue)
      } catch {
        // Keep as string if not valid JSON
      }

      await setStateItem(selectedGroupId, item.key, value)
      dispatchEdit({ type: 'CLOSE_EDIT' })
      refetchItems()
    } catch {
      // Handle error
    } finally {
      dispatchAddModal({ type: 'SAVE_DONE' })
    }
  }

  const copyToClipboard = async (text: string, id: string) => {
    await navigator.clipboard.writeText(text)
    setCopiedId(id)
    setTimeout(() => setCopiedId(null), 2000)
  }

  const sortedItems = useMemo(() => {
    return [...items].sort((a, b) => {
      const aVal = sortField === 'key' ? a.key : a.type
      const bVal = sortField === 'key' ? b.key : b.type
      const comparison = aVal.localeCompare(bVal)
      return sortDirection === 'asc' ? comparison : -comparison
    })
  }, [items, sortField, sortDirection])

  const totalItemPages = Math.max(1, Math.ceil(sortedItems.length / itemsPageSize))
  const paginatedItems = useMemo(() => {
    const start = (itemsPage - 1) * itemsPageSize
    return sortedItems.slice(start, start + itemsPageSize)
  }, [sortedItems, itemsPage, itemsPageSize])

  const toggleSort = (field: 'key' | 'type') => {
    dispatchFilter({ type: 'SET_SORT', field })
  }

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-3 md:py-4 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="font-sans font-semibold text-lg tracking-tight flex items-center gap-2">
            <Folder className="w-5 h-5 text-blue-400" />
            States
          </h1>
          <div className="text-[10px] md:text-xs text-muted bg-dark-gray/50 px-2 py-0.5 md:py-1 rounded hidden sm:block">
            Key-Value Store
          </div>
          <div className="flex items-center gap-1.5 md:gap-2 px-1.5 md:px-2 py-0.5 md:py-1 rounded bg-dark-gray/50 text-[10px] md:text-xs text-muted">
            <Folder className="w-3 h-3" />
            <span>{groups.length} groups</span>
          </div>
        </div>

        <div className="flex items-center gap-1.5 md:gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowMobileSidebar(!showMobileSidebar)}
            className="h-7 text-xs md:hidden"
          >
            <Folder className="w-3 h-3 mr-1" />
            Groups
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => refetchGroups()}
            disabled={loading}
            className="h-6 md:h-7 text-[10px] md:text-xs text-muted hover:text-foreground px-2"
          >
            <RefreshCw
              className={`w-3 h-3 md:w-3.5 md:h-3.5 md:mr-1.5 ${loading ? 'animate-spin' : ''}`}
            />
            <span className="hidden md:inline">Refresh</span>
          </Button>
        </div>
      </div>

      {/* Mobile sidebar overlay */}
      {showMobileSidebar && (
        // biome-ignore lint/a11y/noStaticElementInteractions: click-away overlay with keyboard support
        <div
          role="presentation"
          className="md:hidden fixed inset-0 z-40 bg-black/60"
          onClick={() => setShowMobileSidebar(false)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') setShowMobileSidebar(false)
          }}
        />
      )}

      <div
        className={`flex-1 grid overflow-hidden
        ${
          selectedItem
            ? 'grid-cols-1 md:grid-cols-[240px_1fr] lg:grid-cols-[280px_1fr_320px]'
            : 'grid-cols-1 md:grid-cols-[240px_1fr] lg:grid-cols-[280px_1fr]'
        }`}
      >
        {/* Left Sidebar - State Groups */}
        <div
          className={`
          flex flex-col h-full overflow-hidden border-r border-border bg-dark-gray/20
          fixed md:relative inset-y-0 left-0 z-50 w-[280px] md:w-auto
          transform transition-transform duration-300 ease-in-out
          ${showMobileSidebar ? 'translate-x-0' : '-translate-x-full md:translate-x-0'}
        `}
        >
          <div className="p-2 md:p-3 border-b border-border">
            <div className="flex items-center justify-between md:hidden mb-2">
              <span className="text-xs font-semibold">State Groups</span>
              <button
                type="button"
                onClick={() => setShowMobileSidebar(false)}
                className="p-1 hover:bg-dark-gray rounded"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="relative">
              <Search className="absolute left-2.5 md:left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 md:w-4 md:h-4 text-muted" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search groups..."
                className="pl-8 md:pl-9 pr-8 md:pr-9 h-8 md:h-9 text-xs md:text-sm"
              />
              {searchQuery && (
                <button
                  type="button"
                  onClick={() => setSearchQuery('')}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
                >
                  <X className="w-4 h-4" />
                </button>
              )}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto py-2">
            {loading ? (
              <div className="space-y-2 px-2 py-2">
                <Skeleton className="h-9 w-full" />
                <Skeleton className="h-9 w-full" />
                <Skeleton className="h-9 w-full" />
                <Skeleton className="h-9 w-full" />
              </div>
            ) : filteredGroups.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 px-4">
                <div className="w-12 h-12 mb-3 rounded-[var(--radius-lg)] bg-elevated border border-border-subtle flex items-center justify-center">
                  <Folder className="w-6 h-6 text-muted" />
                </div>
                <div className="font-sans font-semibold text-base text-foreground mb-1">
                  No groups found
                </div>
                <div className="font-sans text-[13px] text-secondary text-center">
                  {searchQuery ? 'Try a different search' : 'Create groups by setting state values'}
                </div>
              </div>
            ) : (
              <div className="space-y-0.5 px-2">
                {filteredGroups.map((group) => (
                  <button
                    key={group.id}
                    type="button"
                    onClick={() => handleSelectGroup(group.id)}
                    className={`w-full flex items-center gap-2 px-3 py-2 text-left transition-colors rounded-md
                      ${
                        selectedGroupId === group.id
                          ? 'bg-blue-500/10 text-blue-400 border-l-2 border-blue-400'
                          : 'text-foreground/80 hover:bg-dark-gray/50'
                      }
                    `}
                  >
                    <Folder
                      className={`h-4 w-4 shrink-0 ${selectedGroupId === group.id ? 'text-blue-400' : 'text-muted'}`}
                    />
                    <div className="flex-1 min-w-0">
                      <div
                        className={`text-sm font-medium truncate ${selectedGroupId === group.id ? 'text-blue-400' : ''}`}
                      >
                        {group.id}
                      </div>
                    </div>
                    <span
                      className={`text-[10px] px-1.5 py-0.5 rounded ${
                        selectedGroupId === group.id
                          ? 'bg-blue-500/20 text-blue-300'
                          : group.count > 0
                            ? 'bg-dark-gray text-muted'
                            : 'bg-dark-gray/50 text-muted/50'
                      }`}
                    >
                      {group.count}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Main Content - Items Table or Empty State */}
        {!selectedGroupId ? (
          <div className="flex flex-col items-center justify-center h-full bg-background">
            <div className="text-center max-w-md px-8">
              <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-blue-500/10 mb-6">
                <Folder className="w-8 h-8 text-blue-400" />
              </div>
              <h2 className="font-sans font-semibold text-xl mb-2">Select a Group</h2>
              <p className="font-sans text-sm text-secondary mb-6">
                Choose a group from the sidebar to view and manage its key-value data.
              </p>
              <div className="text-left bg-elevated rounded-[var(--radius-lg)] border border-border-subtle p-4 text-xs">
                <div className="font-medium mb-2 text-foreground">Groups contain:</div>
                <ul className="space-y-1.5 text-muted">
                  <li className="flex items-center gap-2">
                    <Key className="w-3.5 h-3.5 text-yellow" />
                    <span>
                      <strong className="text-foreground">Items</strong> - Key-value pairs for your
                      data
                    </span>
                  </li>
                </ul>
              </div>
              {groups.length > 0 && (
                <div className="mt-6 text-xs text-muted">{groups.length} groups available</div>
              )}
            </div>
          </div>
        ) : (
          <div className="flex flex-col h-full overflow-hidden bg-background">
            {/* Group Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-dark-gray/30">
              <div className="flex items-center gap-3 min-w-0">
                <div className="p-2 rounded-lg bg-blue-500/10">
                  <Folder className="h-5 w-5 text-blue-400" />
                </div>
                <div className="min-w-0">
                  <h2 className="font-sans text-base font-semibold text-foreground truncate capitalize">
                    {selectedGroupId}
                  </h2>
                  <div className="flex items-center gap-1.5 text-xs text-muted">
                    <Key className="h-3 w-3" />
                    <span>{items.length} items</span>
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => refetchItems()}
                  disabled={loadingItems}
                  className="h-7 w-7 p-0"
                >
                  <RefreshCw className={`w-4 h-4 ${loadingItems ? 'animate-spin' : ''}`} />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedGroupId(null)}
                  className="h-7 w-7 p-0"
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
            </div>

            {/* Items Header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-dark-gray/10">
              <div className="flex items-center gap-2 text-xs text-muted">
                <Key className="w-3.5 h-3.5" />
                <span>{items.length} items</span>
              </div>

              <Button
                variant="outline"
                size="sm"
                onClick={() => dispatchAddModal({ type: 'OPEN_ADD_MODAL' })}
                className="h-7 text-xs gap-1.5"
              >
                <Plus className="w-3 h-3" />
                Add Item
              </Button>
            </div>

            {/* Items Table */}
            <div className="flex flex-col flex-1 overflow-hidden">
              <div className="flex-1 overflow-auto">
                {loadingItems ? (
                  <div className="space-y-2 p-4">
                    <Skeleton className="h-8 w-full" />
                    <Skeleton className="h-10 w-full" />
                    <Skeleton className="h-10 w-full" />
                    <Skeleton className="h-10 w-full" />
                    <Skeleton className="h-10 w-full" />
                  </div>
                ) : selectedGroupId ? (
                  items.length > 0 ? (
                    <table className="w-full">
                      <thead className="sticky top-0 bg-dark-gray/80 backdrop-blur-sm">
                        <tr className="border-b border-border">
                          <th
                            className="text-left font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] px-4 py-2 cursor-pointer hover:text-foreground"
                            onClick={() => toggleSort('key')}
                          >
                            <div className="flex items-center gap-1">
                              Key
                              {sortField === 'key' ? (
                                sortDirection === 'asc' ? (
                                  <ArrowUp className="w-3 h-3" />
                                ) : (
                                  <ArrowDown className="w-3 h-3" />
                                )
                              ) : (
                                <ArrowUpDown className="w-3 h-3 opacity-30" />
                              )}
                            </div>
                          </th>
                          <th
                            className="text-left font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] px-4 py-2 cursor-pointer hover:text-foreground"
                            onClick={() => toggleSort('type')}
                          >
                            <div className="flex items-center gap-1">
                              Type
                              {sortField === 'type' ? (
                                sortDirection === 'asc' ? (
                                  <ArrowUp className="w-3 h-3" />
                                ) : (
                                  <ArrowDown className="w-3 h-3" />
                                )
                              ) : (
                                <ArrowUpDown className="w-3 h-3 opacity-30" />
                              )}
                            </div>
                          </th>
                          <th className="text-left font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] px-4 py-2">
                            Preview
                          </th>
                          <th className="w-20"></th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-border/30">
                        {paginatedItems.map((item) => (
                          <tr
                            key={item.key}
                            onClick={() => setSelectedItem(item)}
                            className={`cursor-pointer transition-colors ${
                              selectedItem?.key === item.key ? 'bg-blue-500/10' : 'hover:bg-hover'
                            }`}
                          >
                            <td className="px-4 py-3">
                              <div className="flex items-center gap-2">
                                <Key className="w-3.5 h-3.5 text-blue-400 shrink-0" />
                                <span className="font-mono text-[13px] font-medium truncate max-w-[200px]">
                                  {item.key}
                                </span>
                              </div>
                            </td>
                            <td className="px-4 py-3">
                              <Badge variant="outline" className="text-[10px]">
                                {item.type}
                              </Badge>
                            </td>
                            <td className="px-4 py-3">
                              <span className="font-mono text-[13px] text-muted truncate block max-w-[300px]">
                                {JSON.stringify(item.value).slice(0, 50)}
                                {JSON.stringify(item.value).length > 50 && '...'}
                              </span>
                            </td>
                            <td className="px-4 py-3">
                              <div className="flex items-center gap-1">
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    copyToClipboard(JSON.stringify(item.value, null, 2), item.key)
                                  }}
                                  className="p-1.5 rounded hover:bg-dark-gray"
                                  title="Copy JSON"
                                >
                                  {copiedId === item.key ? (
                                    <Check className="w-3.5 h-3.5 text-success" />
                                  ) : (
                                    <Copy className="w-3.5 h-3.5 text-muted" />
                                  )}
                                </button>
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    handleDeleteItem(item)
                                  }}
                                  className="p-1.5 rounded hover:bg-dark-gray"
                                  title="Delete"
                                >
                                  <Trash2 className="w-3.5 h-3.5 text-error" />
                                </button>
                              </div>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  ) : (
                    <EmptyState
                      icon={Database}
                      title="No state entries"
                      description="Add key-value pairs using the SDK or the button above"
                      action={{
                        label: 'Add Entry',
                        onClick: () => dispatchAddModal({ type: 'OPEN_ADD_MODAL' }),
                      }}
                    />
                  )
                ) : (
                  <div className="flex flex-col items-center justify-center h-64">
                    <Folder className="h-12 w-12 text-muted/50 mb-4" />
                    <p className="text-sm text-muted">Select a group to view items</p>
                  </div>
                )}
              </div>

              {/* Pagination */}
              {selectedGroupId && items.length > 0 && sortedItems.length > 0 && (
                <div className="flex-shrink-0 bg-background/95 backdrop-blur border-t border-border px-3 py-2">
                  <Pagination
                    currentPage={itemsPage}
                    totalPages={totalItemPages}
                    totalItems={sortedItems.length}
                    pageSize={itemsPageSize}
                    onPageChange={(page) => dispatchPagination({ type: 'SET_PAGE', page })}
                    onPageSizeChange={(pageSize) =>
                      dispatchPagination({ type: 'SET_PAGE_SIZE', pageSize })
                    }
                    pageSizeOptions={[25, 50, 100]}
                  />
                </div>
              )}
            </div>
          </div>
        )}

        {/* Right Sidebar - Item Details */}
        {selectedItem && (
          <div className="flex flex-col h-full overflow-hidden border-l border-border bg-elevated/50">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <div className="flex items-center gap-2 min-w-0">
                <Key className="w-4 h-4 text-blue-400 shrink-0" />
                <span className="font-mono text-[13px] font-medium truncate">
                  {selectedItem.key}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setSelectedItem(null)}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="flex-1 overflow-auto p-4">
              <div className="space-y-4">
                <div>
                  <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-2">
                    Key
                  </div>
                  <div className="font-mono text-[13px] bg-elevated p-2 rounded-[var(--radius-md)]">
                    {selectedItem.key}
                  </div>
                </div>

                <div>
                  <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-2">
                    Type
                  </div>
                  <Badge variant="outline">{selectedItem.type}</Badge>
                </div>

                <div>
                  <div className="flex items-center justify-between mb-2">
                    <span className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em]">
                      Value
                    </span>
                    <div className="flex items-center gap-1">
                      {editingItem === selectedItem.key ? (
                        <>
                          <button
                            type="button"
                            onClick={() => handleEditItem(selectedItem)}
                            disabled={saving}
                            className="p-1 rounded hover:bg-dark-gray text-success"
                          >
                            <Save className="w-3.5 h-3.5" />
                          </button>
                          <button
                            type="button"
                            onClick={() => dispatchEdit({ type: 'CLOSE_EDIT' })}
                            className="p-1 rounded hover:bg-dark-gray"
                          >
                            <X className="w-3.5 h-3.5" />
                          </button>
                        </>
                      ) : (
                        <>
                          <button
                            type="button"
                            onClick={() =>
                              dispatchEdit({
                                type: 'OPEN_EDIT',
                                itemKey: selectedItem.key,
                                value: JSON.stringify(selectedItem.value, null, 2),
                              })
                            }
                            className="p-1 rounded hover:bg-dark-gray"
                          >
                            <Edit2 className="w-3.5 h-3.5 text-muted" />
                          </button>
                          <button
                            type="button"
                            onClick={() =>
                              copyToClipboard(
                                JSON.stringify(selectedItem.value, null, 2),
                                `detail-${selectedItem.key}`,
                              )
                            }
                            className="p-1 rounded hover:bg-dark-gray"
                          >
                            {copiedId === `detail-${selectedItem.key}` ? (
                              <Check className="w-3.5 h-3.5 text-success" />
                            ) : (
                              <Copy className="w-3.5 h-3.5 text-muted" />
                            )}
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                  {editingItem === selectedItem.key ? (
                    <textarea
                      value={editValue}
                      onChange={(e) =>
                        dispatchEdit({ type: 'SET_EDIT_VALUE', value: e.target.value })
                      }
                      className="w-full h-64 px-3 py-2 bg-dark-gray border border-border rounded-md font-mono text-xs resize-none focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                  ) : (
                    <div className="p-3 rounded-[var(--radius-lg)] bg-elevated overflow-x-auto max-h-[400px] overflow-y-auto">
                      <JsonViewer data={selectedItem.value} collapsed={false} maxDepth={6} />
                    </div>
                  )}
                </div>

                <div className="pt-4 border-t border-border">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => handleDeleteItem(selectedItem)}
                    className="w-full gap-1.5 border-error/50 text-error hover:bg-error/10"
                  >
                    <Trash2 className="w-3 h-3" />
                    Delete Item
                  </Button>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Add Item Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-background border border-border-subtle rounded-[var(--radius-lg)] shadow-xl w-full max-w-md">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <h3 className="font-sans font-semibold">Add State Item</h3>
              <button
                type="button"
                onClick={() => dispatchAddModal({ type: 'CLOSE_ADD_MODAL' })}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <span className="text-xs text-muted block mb-1.5">Group</span>
                <div className="flex items-center gap-2 text-sm font-mono bg-dark-gray/50 px-3 py-2 rounded">
                  <Folder className="w-4 h-4 text-blue-400" />
                  <span className="text-blue-400">{selectedGroupId}</span>
                </div>
              </div>

              <div>
                <label htmlFor="state-key" className="text-xs text-muted block mb-1.5">
                  Key
                </label>
                <Input
                  id="state-key"
                  value={newKey}
                  onChange={(e) =>
                    dispatchAddModal({ type: 'SET_ADD_FORM', key: e.target.value, value: newValue })
                  }
                  placeholder="item-key"
                  className="font-mono"
                />
              </div>

              <div>
                <label htmlFor="state-value" className="text-xs text-muted block mb-1.5">
                  Value (JSON or string)
                </label>
                <textarea
                  id="state-value"
                  value={newValue}
                  onChange={(e) =>
                    dispatchAddModal({ type: 'SET_ADD_FORM', key: newKey, value: e.target.value })
                  }
                  placeholder='{"key": "value"}'
                  className="w-full h-32 px-3 py-2 bg-dark-gray border border-border rounded-md font-mono text-sm resize-none focus:outline-none focus:ring-1 focus:ring-blue-500"
                />
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => dispatchAddModal({ type: 'CLOSE_ADD_MODAL' })}
              >
                Cancel
              </Button>
              <Button
                variant="accent"
                size="sm"
                onClick={handleAddItem}
                disabled={!newKey || saving}
                className="gap-1.5"
              >
                {saving ? (
                  <RefreshCw className="w-3 h-3 animate-spin" />
                ) : (
                  <Send className="w-3 h-3" />
                )}
                Save
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
