'use client'

import { Check, Plus, Tag, X } from 'lucide-react'
import { useState } from 'react'

interface AttributesFilterProps {
  value: [string, string][]
  onChange: (attrs: [string, string][]) => void
}

const COMMON_ATTRIBUTES = [
  'http.request.method',
  'http.response.status_code',
  'http.route',
  'url.path',
  'code.file.path',
  'code.module.name',
  'thread.name',
]

let _entryId = 0
type DraftEntry = { id: number; key: string; val: string }

const toDraftEntries = (pairs: [string, string][]): DraftEntry[] =>
  pairs.map(([key, val]) => ({ id: ++_entryId, key, val }))

const toValuePairs = (entries: DraftEntry[]): [string, string][] =>
  entries.map(({ key, val }) => [key, val])

export function AttributesFilter({ value, onChange }: AttributesFilterProps) {
  const [draft, setDraft] = useState<DraftEntry[]>(() => toDraftEntries(value))
  const [isDirty, setIsDirty] = useState(false)
  const [prevValue, setPrevValue] = useState(value)

  if (prevValue !== value) {
    setPrevValue(value)
    setDraft(toDraftEntries(value))
    setIsDirty(false)
  }

  const updateDraft = (newDraft: DraftEntry[]) => {
    setDraft(newDraft)
    setIsDirty(true)
  }

  const handleAdd = () => {
    updateDraft([...draft, { id: ++_entryId, key: '', val: '' }])
  }

  const handleRemove = (id: number) => {
    updateDraft(draft.filter((e) => e.id !== id))
  }

  const handleKeyChange = (id: number, key: string) => {
    updateDraft(draft.map((e) => (e.id === id ? { ...e, key } : e)))
  }

  const handleValueChange = (id: number, val: string) => {
    updateDraft(draft.map((e) => (e.id === id ? { ...e, val } : e)))
  }

  const handleSuggestionClick = (key: string) => {
    updateDraft([...draft, { id: ++_entryId, key, val: '' }])
  }

  const handleApply = () => {
    const filtered = draft.filter(({ key }) => key.trim() !== '')
    onChange(toValuePairs(filtered))
    setIsDirty(false)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && isDirty) {
      handleApply()
    }
  }

  return (
    <div className="space-y-2">
      {draft.length === 0 ? (
        <div className="text-xs text-[#5B5B5B] italic font-mono">
          Filter by span attributes (e.g. http.request.method = POST)
        </div>
      ) : (
        <div className="space-y-2">
          {draft.map(({ id, key, val }) => (
            <div
              key={id}
              className="group flex items-center gap-2 bg-[#0A0A0A] border border-[#1D1D1D] rounded-md p-2 hover:border-[#2D2D2D] transition-colors"
            >
              <input
                type="text"
                placeholder="key"
                value={key}
                onChange={(e) => handleKeyChange(id, e.target.value)}
                onKeyDown={handleKeyDown}
                className="flex-1 bg-transparent border-none text-xs text-[#F4F4F4] placeholder-[#5B5B5B] focus:outline-none font-mono"
              />
              <span className="text-[#5B5B5B] text-xs font-mono">=</span>
              <input
                type="text"
                placeholder="value"
                value={val}
                onChange={(e) => handleValueChange(id, e.target.value)}
                onKeyDown={handleKeyDown}
                className="flex-1 bg-transparent border-none text-xs text-[#F4F4F4] placeholder-[#5B5B5B] focus:outline-none font-mono"
              />
              <button
                type="button"
                onClick={() => handleRemove(id)}
                className="p-1 text-[#5B5B5B] hover:text-red-400 hover:bg-[#141414] rounded transition-all opacity-0 group-hover:opacity-100"
                title="Remove"
              >
                <X className="w-3 h-3" />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="flex items-center justify-between pt-1">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleAdd}
            className="flex items-center gap-1 px-2 py-1 text-xs font-mono text-[#F3F724] hover:bg-[#141414] rounded transition-colors"
          >
            <Plus className="w-3 h-3" />
            Add
          </button>

          {isDirty && (
            <button
              type="button"
              onClick={handleApply}
              className="flex items-center gap-1 px-2.5 py-1 text-[11px] font-mono bg-yellow/10 border border-yellow/30 text-yellow rounded hover:bg-yellow/20 transition-colors"
            >
              <Check className="w-3 h-3" />
              Apply
            </button>
          )}
        </div>

        {draft.length === 0 && (
          <div className="flex items-center gap-1 flex-wrap">
            {COMMON_ATTRIBUTES.map((attr) => (
              <button
                key={attr}
                type="button"
                onClick={() => handleSuggestionClick(attr)}
                className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-mono text-[#9CA3AF] bg-[#0A0A0A] border border-[#1D1D1D] rounded hover:border-[#F3F724] hover:text-[#F3F724] transition-colors"
                title={`Add ${attr}`}
              >
                <Tag className="w-2.5 h-2.5" />
                {attr}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
