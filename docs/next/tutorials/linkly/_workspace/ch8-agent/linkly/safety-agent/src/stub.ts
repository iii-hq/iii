// Deterministic stand-in for provider::anthropic::complete used by tests and by
// developers who don't have ANTHROPIC_API_KEY set. The shape mirrors the real
// AssistantMessage: a content array with optional tool_use blocks.
//
// Behaviour:
// - First call: inspect the URL.
// - After the tool_result comes back, decide by URL substring:
//     contains "malware" → quarantine
//     contains "phishing" → propose_delete
//     otherwise          → allow

type AnthropicMessage = { role: 'user' | 'assistant'; content: unknown }

function lastUserContext(messages: AnthropicMessage[]): { url: string; toolResult: string | null } {
  let url = ''
  let toolResult: string | null = null
  for (const m of messages) {
    if (m.role === 'user' && typeof m.content === 'string') {
      const match = m.content.match(/url:\s*(\S+)/)
      if (match) url = match[1]
    }
    if (m.role === 'user' && Array.isArray(m.content)) {
      for (const block of m.content) {
        if (block && typeof block === 'object' && (block as { type?: unknown }).type === 'tool_result') {
          const tr = block as { content?: unknown }
          if (typeof tr.content === 'string') toolResult = tr.content
        }
      }
    }
  }
  return { url, toolResult }
}

function toolUse(name: string, input: Record<string, unknown>) {
  return { type: 'tool_use', id: `stub_${Math.random().toString(36).slice(2, 10)}`, name, input }
}

export function stubDecide(messages: AnthropicMessage[]): { content: unknown; stop_reason: string } {
  const { url, toolResult } = lastUserContext(messages)
  if (toolResult === null) {
    return { content: [toolUse('inspect_url', { url })], stop_reason: 'tool_use' }
  }
  if (/malware/i.test(url)) {
    return {
      content: [toolUse('quarantine', { reason: 'url contains "malware"' })],
      stop_reason: 'tool_use',
    }
  }
  if (/phishing/i.test(url)) {
    return {
      content: [toolUse('propose_delete', { reason: 'url contains "phishing"' })],
      stop_reason: 'tool_use',
    }
  }
  return { content: [toolUse('allow', { reason: 'no suspicious markers' })], stop_reason: 'tool_use' }
}
