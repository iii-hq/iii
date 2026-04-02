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

export async function fetchStateItems(
  groupId: string,
): Promise<{ items: StateItem[]; count: number }> {
  const res = await fetch(`${getDevtoolsApi()}/states/group`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ scope: groupId }),
  })
  if (!res.ok) throw new Error('Failed to fetch state items')
  const data = await unwrapResponse<{ items: unknown[] }>(res)
  const items: StateItem[] = (data.items || []).map((item: unknown, index: number) => {
    const typedItem = item as Record<string, unknown>
    return {
      groupId,
      key: (typedItem.id as string) || `item-${index}`,
      value: item,
      type: typeof item === 'object' ? 'object' : typeof item,
      timestamp: Date.now(),
    }
  })
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
