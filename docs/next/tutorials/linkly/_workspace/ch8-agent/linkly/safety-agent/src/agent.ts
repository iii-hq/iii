// The agent's tool definitions and the loop driver. The actual tool effects
// (sandbox calls, quarantine, propose_delete) live in index.ts; this file only
// describes the tools to the model and routes its decisions.

export type Tool = {
  name: 'inspect_url' | 'quarantine' | 'propose_delete' | 'allow'
  description: string
  parameters: Record<string, unknown>
}

export const TOOLS: Tool[] = [
  {
    name: 'inspect_url',
    description:
      'Fetch the target URL inside an ephemeral iii-sandbox and return the HTTP status, redirect chain, and a short body excerpt. Use this to investigate before classifying.',
    parameters: {
      type: 'object',
      properties: { url: { type: 'string' } },
      required: ['url'],
    },
  },
  {
    name: 'quarantine',
    description:
      'Mark the link malicious. Resolves stop returning a URL; auto-applied, no human review. Use for clear-cut malware, phishing, or known-bad destinations.',
    parameters: {
      type: 'object',
      properties: { reason: { type: 'string' } },
      required: ['reason'],
    },
  },
  {
    name: 'propose_delete',
    description:
      'Ask a human operator to confirm deletion. The browser admin shows a confirm prompt; the link is only removed if confirmed. Use for suspicious but ambiguous links.',
    parameters: {
      type: 'object',
      properties: { reason: { type: 'string' } },
      required: ['reason'],
    },
  },
  {
    name: 'allow',
    description: 'Conclude the investigation; the link is fine. End the turn.',
    parameters: {
      type: 'object',
      properties: { reason: { type: 'string' } },
      required: ['reason'],
    },
  },
]

export const SYSTEM_PROMPT = `You are Linkly's link-safety agent. A new shortened link was created. Your job:

1. Decide whether to investigate further with inspect_url, or whether you already know enough.
2. Reach one terminal decision per turn: quarantine, propose_delete, or allow.

Quarantine is auto-applied (no human in the loop), so only use it when you are confident the destination is malicious. propose_delete sends a confirm prompt to a human operator — use it for ambiguous but suspicious cases. allow ends the turn with no action.

Always finish by calling exactly one of: quarantine, propose_delete, or allow.`

// A single tool_use block from the model's response.
export type ToolCall = { id: string; name: Tool['name']; input: Record<string, unknown> }

// The terminal verdict the agent landed on, plus its stated reason.
export type Verdict =
  | { action: 'allow'; reason: string }
  | { action: 'quarantine'; reason: string }
  | { action: 'propose_delete'; reason: string }

// Picks one tool_use from an assistant message; null if there is none.
export function pickToolCall(content: unknown): ToolCall | null {
  if (!Array.isArray(content)) return null
  for (const block of content) {
    if (
      block &&
      typeof block === 'object' &&
      (block as { type?: unknown }).type === 'tool_use' &&
      typeof (block as { id?: unknown }).id === 'string' &&
      typeof (block as { name?: unknown }).name === 'string'
    ) {
      const b = block as { id: string; name: string; input?: Record<string, unknown> }
      return { id: b.id, name: b.name as Tool['name'], input: b.input ?? {} }
    }
  }
  return null
}

// Reads `reason` off a tool_use's input safely.
export function reasonOf(input: Record<string, unknown>): string {
  const r = input.reason
  return typeof r === 'string' ? r : ''
}
