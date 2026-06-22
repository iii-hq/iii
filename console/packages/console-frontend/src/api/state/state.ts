import { getDevtoolsApi } from '../config'
import { unwrapResponse } from '../utils'

// ============================================================================
// State Types
// ============================================================================

export interface StateItem {
  groupId: string
  key: string
  value: unknown
  type: string
  timestamp?: number
}

export interface StateGroup {
  id: string
  count: number
}

// ============================================================================
// State Functions (used functions only)
// ============================================================================

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value)
}

function nonEmptyString(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function stateItemValue(item: Record<string, unknown>): unknown {
  if ('value' in item && (nonEmptyString(item.key) || nonEmptyString(item.state_key))) {
    return item.value
  }

  return item
}

function makeStateItem(groupId: string, key: string, value: unknown): StateItem {
  return {
    groupId,
    key,
    value,
    type: typeof value === 'object' ? 'object' : typeof value,
    timestamp: Date.now(),
  }
}

function fallbackKey(index: number): string {
  return `(missing key ${index})`
}

function isGeneratedItemKey(key: string): boolean {
  return /^item-\d+$/.test(key)
}

function normalizeStateItem(groupId: string, item: unknown, index: number): StateItem {
  if (Array.isArray(item) && item.length >= 2) {
    const key = nonEmptyString(item[0])
    if (key) return makeStateItem(groupId, key, item[1])
  }

  if (isRecord(item)) {
    const key =
      nonEmptyString(item.key) ?? nonEmptyString(item.state_key) ?? nonEmptyString(item.id)
    return makeStateItem(groupId, key ?? fallbackKey(index), stateItemValue(item))
  }

  return makeStateItem(groupId, fallbackKey(index), item)
}

function normalizeStateItems(groupId: string, rawItems: unknown): StateItem[] {
  if (Array.isArray(rawItems)) {
    return rawItems.map((item, index) => normalizeStateItem(groupId, item, index))
  }

  if (isRecord(rawItems)) {
    return Object.entries(rawItems).map(([key, value], index) => {
      const embeddedKey = isRecord(value)
        ? (nonEmptyString(value.key) ?? nonEmptyString(value.state_key) ?? nonEmptyString(value.id))
        : null
      const entryKey = key && !isGeneratedItemKey(key) ? key : null
      const itemValue = isRecord(value) ? stateItemValue(value) : value
      return makeStateItem(groupId, embeddedKey ?? entryKey ?? fallbackKey(index), itemValue)
    })
  }

  return []
}

export async function fetchStateItems(
  groupId: string,
): Promise<{ items: StateItem[]; count: number }> {
  const res = await fetch(`${getDevtoolsApi()}/states/group`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ scope: groupId }),
  })
  if (!res.ok) throw new Error('Failed to fetch state items')
  const data = await unwrapResponse<{ items?: unknown } | Record<string, unknown> | unknown[]>(res)
  const rawItems = Array.isArray(data) || !isRecord(data) || !('items' in data) ? data : data.items
  const items = normalizeStateItems(groupId, rawItems)
  return { items, count: items.length }
}

export async function fetchStateGroups(): Promise<{
  groups: StateGroup[]
  count: number
}> {
  const res = await fetch(`${getDevtoolsApi()}/states/groups`, {
    method: 'GET',
    headers: { 'Content-Type': 'application/json' },
  })
  if (!res.ok) throw new Error('Failed to fetch state groups')
  const data = await unwrapResponse<{ groups: StateGroup[] }>(res)
  return { groups: data.groups || [], count: (data.groups || []).length }
}

export async function setStateItem(groupId: string, key: string, value: unknown): Promise<void> {
  const res = await fetch(`${getDevtoolsApi()}/states/${encodeURIComponent(groupId)}/item`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ key, value }),
  })
  if (!res.ok) throw new Error('Failed to set state item')
}

export async function deleteStateItem(groupId: string, key: string): Promise<void> {
  const res = await fetch(
    `${getDevtoolsApi()}/states/${encodeURIComponent(groupId)}/item/${encodeURIComponent(key)}`,
    {
      method: 'DELETE',
    },
  )
  if (!res.ok) throw new Error('Failed to delete state item')
}
