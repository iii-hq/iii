import { describe, expect, it } from 'vitest'
import { pickToolCall, reasonOf } from '../src/agent.js'
import { stubDecide } from '../src/stub.js'

describe('pickToolCall', () => {
  it('returns null when content is not an array', () => {
    expect(pickToolCall('hi')).toBeNull()
    expect(pickToolCall(null)).toBeNull()
  })

  it('picks the first tool_use block', () => {
    const content = [
      { type: 'text', text: 'thinking…' },
      { type: 'tool_use', id: 'x', name: 'quarantine', input: { reason: 'r' } },
    ]
    expect(pickToolCall(content)).toEqual({ id: 'x', name: 'quarantine', input: { reason: 'r' } })
  })

  it('returns null if there is no tool_use', () => {
    expect(pickToolCall([{ type: 'text', text: 'hi' }])).toBeNull()
  })
})

describe('reasonOf', () => {
  it('returns string reason, empty otherwise', () => {
    expect(reasonOf({ reason: 'why' })).toBe('why')
    expect(reasonOf({})).toBe('')
    expect(reasonOf({ reason: 42 })).toBe('')
  })
})

describe('stubDecide', () => {
  const initial = (url: string) => [
    { role: 'user' as const, content: `A new shortened link was just created.\n\ncode: abc\nurl: ${url}\n\nInvestigate and decide.` },
  ]
  const afterInspect = (url: string, result: string) => [
    ...initial(url),
    { role: 'assistant' as const, content: [{ type: 'tool_use', id: 'x', name: 'inspect_url', input: { url } }] },
    { role: 'user' as const, content: [{ type: 'tool_result', tool_use_id: 'x', content: result }] },
  ]

  it('first turn: requests inspect_url', () => {
    const r = stubDecide(initial('https://anything.example'))
    const call = pickToolCall(r.content)
    expect(call?.name).toBe('inspect_url')
  })

  it('after inspect with "malware" url: quarantine', () => {
    const r = stubDecide(afterInspect('https://malware.example', '404'))
    expect(pickToolCall(r.content)?.name).toBe('quarantine')
  })

  it('after inspect with "phishing" url: propose_delete', () => {
    const r = stubDecide(afterInspect('https://phishing.example', '200'))
    expect(pickToolCall(r.content)?.name).toBe('propose_delete')
  })

  it('after inspect with benign url: allow', () => {
    const r = stubDecide(afterInspect('https://iii.dev', '200'))
    expect(pickToolCall(r.content)?.name).toBe('allow')
  })
})
