import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import {
  Activity,
  Check,
  CheckCircle,
  ChevronRight,
  Code2,
  Copy,
  FileJson,
  Eye,
  EyeOff,
  Loader2,
  Play,
  RefreshCw,
  Server,
  X,
  XCircle,
} from 'lucide-react'
import { useEffect, useReducer, useRef } from 'react'
import { z } from 'zod'
import type { FunctionDetail, FunctionInfo } from '@/api'
import {
  fetchFunctionDetail,
  functionsQuery,
  invokeFunction as invokeFunctionApi,
  workersQuery,
} from '@/api'
import { Badge, Button } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { EmptyState } from '@/components/ui/empty-state'
import { JsonViewer } from '@/components/ui/json-viewer'
import { PageHeader } from '@/components/ui/page-header'
import { SearchBar } from '@/components/ui/search-bar'
import { Skeleton } from '@/components/ui/skeleton'

// --- invocation reducer ---
interface InvocationResult {
  success: boolean
  status?: number
  duration?: number
  data?: unknown
  error?: string
}

interface InvocationState {
  invoking: boolean
  invocationResult: InvocationResult | null
  requestBody: string
}

type InvocationAction =
  | { type: 'START_INVOKE' }
  | { type: 'SET_RESULT'; result: InvocationResult }
  | { type: 'CLEAR_RESULT' }
  | { type: 'SET_REQUEST_BODY'; body: string }
  | { type: 'INVOKE_DONE' }

const invocationInitial: InvocationState = {
  invoking: false,
  invocationResult: null,
  requestBody: '{}',
}

function invocationReducer(state: InvocationState, action: InvocationAction): InvocationState {
  switch (action.type) {
    case 'START_INVOKE':
      return { ...state, invoking: true, invocationResult: null }
    case 'SET_RESULT':
      return { ...state, invocationResult: action.result }
    case 'CLEAR_RESULT':
      return { ...state, invocationResult: null }
    case 'SET_REQUEST_BODY':
      return { ...state, requestBody: action.body }
    case 'INVOKE_DONE':
      return { ...state, invoking: false }
    default:
      return state
  }
}

// --- UI reducer ---
interface FunctionsUiState {
  searchQuery: string
  showSystem: boolean
  selectedFunction: FunctionInfo | null
  functionDetail: FunctionDetail | null
  functionDetailLoading: boolean
  functionDetailError: string | null
  schemaDialogOpen: boolean
  copied: string | null
  collapsedGroups: Set<string>
}

type FunctionsUiAction =
  | { type: 'SET_SEARCH_QUERY'; payload: string }
  | { type: 'TOGGLE_SHOW_SYSTEM' }
  | { type: 'SET_SELECTED_FUNCTION'; payload: FunctionInfo | null }
  | { type: 'START_FUNCTION_DETAIL' }
  | { type: 'SET_FUNCTION_DETAIL'; payload: FunctionDetail }
  | { type: 'SET_FUNCTION_DETAIL_ERROR'; payload: string }
  | { type: 'SET_SCHEMA_DIALOG_OPEN'; payload: boolean }
  | { type: 'SET_COPIED'; payload: string | null }
  | { type: 'TOGGLE_GROUP'; payload: string }

function functionsUiReducer(state: FunctionsUiState, action: FunctionsUiAction): FunctionsUiState {
  switch (action.type) {
    case 'SET_SEARCH_QUERY':
      return { ...state, searchQuery: action.payload }
    case 'TOGGLE_SHOW_SYSTEM':
      return { ...state, showSystem: !state.showSystem }
    case 'SET_SELECTED_FUNCTION':
      return {
        ...state,
        selectedFunction: action.payload,
        ...(!action.payload && { schemaDialogOpen: false }),
      }
    case 'START_FUNCTION_DETAIL':
      return {
        ...state,
        functionDetail: null,
        functionDetailLoading: true,
        functionDetailError: null,
      }
    case 'SET_FUNCTION_DETAIL':
      return { ...state, functionDetail: action.payload, functionDetailLoading: false }
    case 'SET_FUNCTION_DETAIL_ERROR':
      return { ...state, functionDetailLoading: false, functionDetailError: action.payload }
    case 'SET_SCHEMA_DIALOG_OPEN':
      return { ...state, schemaDialogOpen: action.payload }
    case 'SET_COPIED':
      return { ...state, copied: action.payload }
    case 'TOGGLE_GROUP': {
      const next = new Set(state.collapsedGroups)
      if (next.has(action.payload)) next.delete(action.payload)
      else next.add(action.payload)
      return { ...state, collapsedGroups: next }
    }
    default:
      return state
  }
}

const functionsSearchSchema = z.object({
  q: z.string().optional(),
})

export const Route = createFileRoute('/functions')({
  validateSearch: functionsSearchSchema,
  component: FunctionsPage,
  loader: ({ context: { queryClient } }) => {
    Promise.allSettled([
      queryClient.prefetchQuery(functionsQuery()),
      queryClient.prefetchQuery(workersQuery),
    ])
  },
})

function FunctionsPage() {
  const { q: qFromSearch } = Route.useSearch()

  const [uiState, dispatchUi] = useReducer(functionsUiReducer, {
    searchQuery: '',
    showSystem: false,
    selectedFunction: null,
    functionDetail: null,
    functionDetailLoading: false,
    functionDetailError: null,
    schemaDialogOpen: false,
    copied: null,
    collapsedGroups: new Set<string>(),
  })
  const {
    searchQuery,
    showSystem,
    selectedFunction,
    functionDetail,
    functionDetailLoading,
    functionDetailError,
    schemaDialogOpen,
    copied,
    collapsedGroups,
  } = uiState

  useEffect(() => {
    if (qFromSearch !== undefined) {
      dispatchUi({ type: 'SET_SEARCH_QUERY', payload: qFromSearch })
    }
  }, [qFromSearch])

  const [invocationState, dispatchInvocation] = useReducer(invocationReducer, invocationInitial)
  const { invoking, invocationResult, requestBody } = invocationState

  // Tracks the currently-selected function id so an in-flight detail fetch can
  // be discarded if the user selects a different function before it resolves.
  const selectedFunctionRef = useRef<string | null>(null)

  const {
    data: functionsData,
    isLoading: loadingFunctions,
    refetch: refetchFunctions,
  } = useQuery(functionsQuery({ include_internal: showSystem }))
  const { refetch: refetchWorkers } = useQuery(workersQuery)

  const functions = functionsData?.functions || []
  const loading = loadingFunctions

  const toggleGroup = (group: string) => {
    dispatchUi({ type: 'TOGGLE_GROUP', payload: group })
  }

  const loadData = () => {
    refetchFunctions()
    refetchWorkers()
  }

  const userFunctions = functions.filter((f) => !f.internal)
  const systemFunctions = functions.filter((f) => f.internal)

  const filteredFunctions = functions.filter((f) => {
    if (!showSystem && f.internal) return false
    if (searchQuery && !f.function_id.toLowerCase().includes(searchQuery.toLowerCase()))
      return false
    return true
  })

  const groupedFunctions = filteredFunctions.reduce(
    (acc, fn) => {
      const parts = fn.function_id.split('::')
      const group = parts.length > 1 ? parts[0].toUpperCase() : 'OTHER'
      if (!acc[group]) acc[group] = []
      acc[group].push(fn)
      return acc
    },
    {} as Record<string, FunctionInfo[]>,
  )

  // Sort functions within each group alphabetically
  for (const group of Object.keys(groupedFunctions)) {
    groupedFunctions[group].sort((a, b) => a.function_id.localeCompare(b.function_id))
  }

  const groups = Object.keys(groupedFunctions).sort((a, b) => {
    if (a === 'OTHER') return 1
    if (b === 'OTHER') return -1
    return a.localeCompare(b)
  })

  const copyToClipboard = (text: string, key: string) => {
    navigator.clipboard.writeText(text)
    dispatchUi({ type: 'SET_COPIED', payload: key })
    setTimeout(() => dispatchUi({ type: 'SET_COPIED', payload: null }), 2000)
  }

  const generateTemplate = (schema: unknown): string => {
    if (!schema || typeof schema !== 'object') return '{}'
    const obj = schema as Record<string, unknown>
    const props = obj.properties as Record<string, Record<string, unknown>> | undefined
    if (!props) return '{}'
    const template: Record<string, unknown> = {}
    for (const [key, prop] of Object.entries(props)) {
      const type = prop.type as string | undefined
      if (type === 'string') template[key] = ''
      else if (type === 'number' || type === 'integer') template[key] = 0
      else if (type === 'boolean') template[key] = false
      else if (type === 'array') template[key] = []
      else if (type === 'object') template[key] = {}
      else template[key] = null
    }
    return JSON.stringify(template, null, 2)
  }

  const invokeFunction = async (fn: FunctionInfo) => {
    dispatchInvocation({ type: 'START_INVOKE' })
    const startTime = Date.now()

    try {
      let input: unknown = {}
      try {
        input = JSON.parse(requestBody)
      } catch {
        dispatchInvocation({
          type: 'SET_RESULT',
          result: { success: false, error: 'Invalid JSON in request body' },
        })
        dispatchInvocation({ type: 'INVOKE_DONE' })
        return
      }

      const result = await invokeFunctionApi(fn.function_id, input)
      const duration = Date.now() - startTime

      dispatchInvocation({
        type: 'SET_RESULT',
        result: { success: result.success, duration, data: result.data, error: result.error },
      })
    } catch (err) {
      dispatchInvocation({
        type: 'SET_RESULT',
        result: {
          success: false,
          duration: Date.now() - startTime,
          error: err instanceof Error ? err.message : 'Invocation failed',
        },
      })
    } finally {
      dispatchInvocation({ type: 'INVOKE_DONE' })
    }
  }

  const handleSelectFunction = (fn: FunctionInfo) => {
    if (selectedFunction?.function_id === fn.function_id) {
      selectedFunctionRef.current = null
      dispatchUi({ type: 'SET_SELECTED_FUNCTION', payload: null })
    } else {
      selectedFunctionRef.current = fn.function_id
      dispatchUi({ type: 'SET_SELECTED_FUNCTION', payload: fn })
      dispatchUi({ type: 'START_FUNCTION_DETAIL' })
      dispatchInvocation({ type: 'CLEAR_RESULT' })
      // The list route returns slim summaries without schemas. Fetch the
      // detail to pre-fill the request body from `request_schema`; fall back to
      // an empty object if the schema is absent or the fetch fails.
      dispatchInvocation({ type: 'SET_REQUEST_BODY', body: '{\n  \n}' })
      fetchFunctionDetail(fn.function_id)
        .then((detail) => {
          if (selectedFunctionRef.current !== fn.function_id) return
          dispatchUi({ type: 'SET_FUNCTION_DETAIL', payload: detail })
          const template = detail.request_schema
            ? generateTemplate(detail.request_schema)
            : '{\n  \n}'
          dispatchInvocation({ type: 'SET_REQUEST_BODY', body: template })
        })
        .catch(() => {
          if (selectedFunctionRef.current !== fn.function_id) return
          dispatchUi({
            type: 'SET_FUNCTION_DETAIL_ERROR',
            payload: 'Unable to load schemas for this function.',
          })
          // Keep the empty-object fallback already set above.
        })
    }
  }

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <PageHeader
        icon={Server}
        title="Functions"
        actions={
          <>
            <Button
              variant={showSystem ? 'accent' : 'ghost'}
              size="sm"
              onClick={() => dispatchUi({ type: 'TOGGLE_SHOW_SYSTEM' })}
              className="h-6 md:h-7 text-[10px] md:text-xs px-2"
            >
              {showSystem ? (
                <Eye className="w-3 h-3 md:mr-1.5" />
              ) : (
                <EyeOff className="w-3 h-3 md:mr-1.5" />
              )}
              <span className={`hidden md:inline ${showSystem ? '' : 'line-through opacity-60'}`}>
                System
              </span>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={loadData}
              disabled={loading}
              className="h-7 text-xs"
            >
              <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${loading ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
          </>
        }
      >
        <Badge variant="success" className="gap-1 text-[10px] md:text-xs">
          <Activity className="w-2.5 h-2.5 md:w-3 md:h-3" />
          {userFunctions.length}
        </Badge>
        {systemFunctions.length > 0 && !showSystem && (
          <span className="text-[10px] md:text-xs text-muted hidden md:inline">
            ({systemFunctions.length} system)
          </span>
        )}
      </PageHeader>

      {/* Search Bar Row */}
      <SearchBar
        value={searchQuery}
        onChange={(value) => dispatchUi({ type: 'SET_SEARCH_QUERY', payload: value })}
        placeholder="Search functions..."
      />

      <div
        className={`flex-1 flex overflow-hidden ${selectedFunction ? 'divide-x divide-border' : ''}`}
      >
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          {loading ? (
            <div className="space-y-3 py-4">
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
            </div>
          ) : filteredFunctions.length === 0 ? (
            searchQuery ? (
              <div className="flex flex-col items-center justify-center py-12">
                <Code2 className="w-12 h-12 text-muted/30 mb-4" />
                <div className="font-sans font-semibold text-base text-foreground mb-1">
                  No functions found
                </div>
                <div className="font-sans text-[13px] text-secondary">
                  Try a different search term
                </div>
              </div>
            ) : (
              <EmptyState
                icon={Server}
                title="No functions registered"
                description="Register functions using the SDK to see them here"
              />
            )
          ) : (
            groups.map((group) => (
              <div key={group}>
                <button
                  type="button"
                  onClick={() => toggleGroup(group)}
                  className="flex items-center gap-2 mb-3 cursor-pointer hover:opacity-80 transition-opacity"
                >
                  <ChevronRight
                    className={`w-3 h-3 text-muted transition-transform duration-150 ${!collapsedGroups.has(group) ? 'rotate-90' : ''}`}
                  />
                  <Badge variant="outline" className="text-[10px] uppercase tracking-wider">
                    {group}
                  </Badge>
                  <span className="text-[10px] text-muted">
                    {groupedFunctions[group].length} functions
                  </span>
                </button>
                {!collapsedGroups.has(group) && (
                  <div className="space-y-1">
                    {groupedFunctions[group].map((fn) => {
                      const isSelected = selectedFunction?.function_id === fn.function_id

                      return (
                        <button
                          key={fn.function_id}
                          type="button"
                          onClick={() => handleSelectFunction(fn)}
                          className={`group flex items-center gap-3 px-3 py-2.5 rounded-[var(--radius-lg)] cursor-pointer transition-all w-full text-left
                          ${
                            isSelected
                              ? 'bg-primary/10 border border-primary/30 ring-1 ring-primary/20'
                              : 'bg-elevated border border-transparent hover:bg-hover hover:border-border'
                          }
                        `}
                        >
                          <div className="shrink-0">
                            <Code2 className="w-4 h-4 text-muted" />
                          </div>

                          <div className="flex-1 min-w-0">
                            <span
                              className={`font-mono text-[13px] font-medium ${isSelected ? 'text-primary' : 'text-yellow'}`}
                            >
                              {fn.function_id}
                            </span>
                          </div>

                          <ChevronRight
                            className={`w-4 h-4 text-muted shrink-0 transition-transform ${isSelected ? 'rotate-90' : ''}`}
                          />
                        </button>
                      )
                    })}
                  </div>
                )}
              </div>
            ))
          )}
        </div>

        {selectedFunction && (
          <div
            className="
            fixed inset-0 z-50 md:relative md:inset-auto
            w-full md:w-[360px] lg:w-[480px] shrink-0
            flex flex-col h-full overflow-hidden bg-background md:bg-dark-gray/20 border-l border-border
          "
          >
            <div className="px-3 md:px-4 py-2 md:py-3 border-b border-border bg-dark-gray/30 space-y-1.5">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 min-w-0">
                  <Code2 className="w-4 h-4 text-muted" />
                  <h2 className="font-medium text-xs md:text-sm truncate">
                    {selectedFunction.function_id}
                  </h2>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    type="button"
                    onClick={() => dispatchUi({ type: 'SET_SCHEMA_DIALOG_OPEN', payload: true })}
                    className="p-1.5 hover:bg-dark-gray rounded transition-colors"
                    title="View request and response schemas"
                    aria-label="View request and response schemas"
                  >
                    <FileJson className="w-3.5 h-3.5 text-muted" />
                  </button>
                  <button
                    type="button"
                    onClick={() => copyToClipboard(selectedFunction.function_id, 'path')}
                    className="p-1.5 hover:bg-dark-gray rounded transition-colors"
                    title="Copy function ID"
                  >
                    {copied === 'path' ? (
                      <Check className="w-3.5 h-3.5 text-success" />
                    ) : (
                      <Copy className="w-3.5 h-3.5 text-muted" />
                    )}
                  </button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      selectedFunctionRef.current = null
                      dispatchUi({ type: 'SET_SELECTED_FUNCTION', payload: null })
                    }}
                    className="h-7 w-7 md:h-6 md:w-6 p-0"
                  >
                    <X className="w-4 h-4" />
                  </Button>
                </div>
              </div>
              {selectedFunction.description && (
                <p className="text-[11px] text-muted leading-relaxed">
                  {selectedFunction.description}
                </p>
              )}
            </div>

            <div className="flex-1 overflow-y-auto p-4 space-y-5">
              <div>
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-3 flex items-center gap-2">
                  <Play className="w-3 h-3" />
                  Invoke Function
                </div>

                <div className="space-y-3">
                  <div>
                    <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-1.5">
                      Input (JSON)
                    </div>
                    <textarea
                      value={requestBody}
                      onChange={(e) =>
                        dispatchInvocation({ type: 'SET_REQUEST_BODY', body: e.target.value })
                      }
                      className="w-full h-24 text-xs font-mono bg-black/40 text-foreground px-3 py-2 rounded border border-border focus:border-primary focus:outline-none resize-none"
                      placeholder='{"key": "value"}'
                    />
                  </div>

                  <Button
                    onClick={() => invokeFunction(selectedFunction)}
                    disabled={invoking}
                    className="w-full h-9"
                  >
                    {invoking ? (
                      <>
                        <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                        Invoking...
                      </>
                    ) : (
                      <>
                        <Play className="w-3.5 h-3.5 mr-2" />
                        Invoke
                      </>
                    )}
                  </Button>
                </div>

                {invocationResult && (
                  <div
                    className={`mt-3 border rounded-lg overflow-hidden ${
                      invocationResult.success
                        ? 'border-success/30 bg-success/5'
                        : 'border-error/30 bg-error/5'
                    }`}
                  >
                    <div
                      className={`flex items-center justify-between px-3 py-2 border-b ${
                        invocationResult.success ? 'border-success/20' : 'border-error/20'
                      }`}
                    >
                      <div className="flex items-center gap-2">
                        {invocationResult.success ? (
                          <CheckCircle className="w-3.5 h-3.5 text-success" />
                        ) : (
                          <XCircle className="w-3.5 h-3.5 text-error" />
                        )}
                        <span
                          className={`text-xs font-medium ${invocationResult.success ? 'text-success' : 'text-error'}`}
                        >
                          {invocationResult.success ? 'Success' : 'Error'}
                        </span>
                      </div>
                      {invocationResult.duration && (
                        <span className="text-[10px] text-muted">
                          {invocationResult.duration}ms
                        </span>
                      )}
                    </div>
                    <div className="p-3 overflow-x-auto max-h-48 overflow-y-auto">
                      {invocationResult.error ? (
                        <pre className="text-[11px] font-mono text-error">
                          {invocationResult.error}
                        </pre>
                      ) : (
                        <JsonViewer data={invocationResult.data} collapsed={false} maxDepth={4} />
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>

            <Dialog
              open={schemaDialogOpen}
              onOpenChange={(open) =>
                dispatchUi({ type: 'SET_SCHEMA_DIALOG_OPEN', payload: open })
              }
            >
              <DialogContent className="w-[80vw] max-w-[80vw] h-[90vh] max-h-[90vh] overflow-hidden flex flex-col">
                <DialogHeader>
                  <DialogTitle className="flex items-center gap-2 pr-6">
                    <FileJson className="h-4 w-4 text-muted" />
                    Function Schemas
                  </DialogTitle>
                  <DialogDescription className="font-mono text-xs break-all">
                    {selectedFunction.function_id}
                  </DialogDescription>
                </DialogHeader>

                <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden grid grid-cols-1 md:grid-cols-2 gap-4 mt-2">
                  {functionDetailLoading ? (
                    <div className="md:col-span-2 space-y-3 py-4">
                      <Skeleton className="h-32 w-full" />
                      <Skeleton className="h-32 w-full" />
                    </div>
                  ) : functionDetailError ? (
                    <div className="md:col-span-2 rounded border border-error/30 bg-error/5 p-4 text-sm text-error">
                      {functionDetailError}
                    </div>
                  ) : (
                    <>
                      <div className="min-w-0 min-h-0 flex flex-col">
                        <h3 className="mb-2 shrink-0 text-xs font-semibold uppercase tracking-[0.04em] text-muted">
                          Request Schema
                        </h3>
                        <div className="min-h-32 flex-1 overflow-y-auto overflow-x-hidden rounded border border-border bg-black/40 p-3">
                          {functionDetail?.request_schema ? (
                            <JsonViewer
                              data={functionDetail.request_schema}
                              collapsed={false}
                              maxDepth={8}
                              className="min-w-0 [overflow-wrap:anywhere] [&_pre]:overflow-x-hidden [&_pre]:whitespace-pre-wrap [&_pre]:break-words"
                            />
                          ) : (
                            <p className="text-xs text-muted">No request schema provided.</p>
                          )}
                        </div>
                      </div>
                      <div className="min-w-0 min-h-0 flex flex-col">
                        <h3 className="mb-2 shrink-0 text-xs font-semibold uppercase tracking-[0.04em] text-muted">
                          Response Schema
                        </h3>
                        <div className="min-h-32 flex-1 overflow-y-auto overflow-x-hidden rounded border border-border bg-black/40 p-3">
                          {functionDetail?.response_schema ? (
                            <JsonViewer
                              data={functionDetail.response_schema}
                              collapsed={false}
                              maxDepth={8}
                              className="min-w-0 [overflow-wrap:anywhere] [&_pre]:overflow-x-hidden [&_pre]:whitespace-pre-wrap [&_pre]:break-words"
                            />
                          ) : (
                            <p className="text-xs text-muted">No response schema provided.</p>
                          )}
                        </div>
                      </div>
                    </>
                  )}
                </div>
              </DialogContent>
            </Dialog>
          </div>
        )}
      </div>
    </div>
  )
}
