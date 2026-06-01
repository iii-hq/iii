import { registerWorker, Logger, TriggerAction } from 'iii-sdk'
import { TOOLS, SYSTEM_PROMPT, pickToolCall, reasonOf } from './agent.js'
import { stubDecide } from './stub.js'

const worker = registerWorker(process.env.III_URL ?? 'ws://localhost:49134', {
  workerName: 'safety-agent',
})
const logger = new Logger()

const SAMPLE_RATE = Number(process.env.SAFETY_SAMPLE_RATE ?? '1')
// Default to the deterministic stub so the tutorial runs without ANTHROPIC_API_KEY.
// Set SAFETY_AGENT_STUB=0 to call provider::anthropic::complete for real.
const STUB = process.env.SAFETY_AGENT_STUB !== '0'
const MODEL = process.env.SAFETY_AGENT_MODEL ?? 'claude-haiku-4-5-20251001'
const MAX_TURNS = 6

// --- LLM call: routes through harness so it gets an iii-observability span ---

type AnthropicMessage = {
  role: 'user' | 'assistant'
  content: unknown
}

async function complete(messages: AnthropicMessage[]): Promise<{ content: unknown; stop_reason: string }> {
  if (STUB) {
    // Deterministic stub for tests. See stub.ts.
    return stubDecide(messages)
  }
  return worker.trigger<
    { model: string; system_prompt: string; messages: AnthropicMessage[]; tools: typeof TOOLS },
    { content: unknown; stop_reason: string }
  >({
    function_id: 'provider::anthropic::complete',
    payload: { model: MODEL, system_prompt: SYSTEM_PROMPT, messages, tools: TOOLS },
    timeoutMs: 60_000,
  })
}

// --- Tool implementations ---

async function inspectUrl(url: string): Promise<string> {
  // Spawn an iii-sandbox just to fetch the URL in isolation. The agent never
  // touches the network from inside this worker; everything goes through the VM.
  const { sandbox_id } = await worker.trigger<{ image: string }, { sandbox_id: string }>({
    function_id: 'sandbox::create',
    payload: { image: 'node' },
  })
  try {
    const exec = await worker.trigger<
      { sandbox_id: string; cmd: string; args: string[]; timeout_ms: number },
      { stdout: string; stderr: string; exit_code: number; success: boolean; timed_out: boolean }
    >({
      function_id: 'sandbox::exec',
      payload: {
        sandbox_id,
        cmd: 'curl',
        args: ['-sIL', '--max-time', '5', '-o', '/dev/null', '-w', '%{http_code} %{redirect_url}\\n', url],
        timeout_ms: 8000,
      },
    })
    if (!exec.success) {
      return `inspect failed: exit=${exec.exit_code} stderr=${exec.stderr.slice(0, 200)}`
    }
    return exec.stdout.trim().slice(0, 400)
  } finally {
    await worker.trigger({ function_id: 'sandbox::stop', payload: { sandbox_id } }).catch(() => {})
  }
}

async function quarantine(code: string, reason: string): Promise<void> {
  await worker.trigger({ function_id: 'link::quarantine', payload: { code, reason } })
}

async function proposeDelete(code: string): Promise<{ confirmed: boolean }> {
  return worker.trigger<{ code: string }, { confirmed: boolean }>({
    function_id: 'link::request_delete',
    payload: { code },
  })
}

// --- Investigate one link: run the tools loop ---

async function investigate(link: { code: string; url: string }): Promise<void> {
  const messages: AnthropicMessage[] = [
    {
      role: 'user',
      content: `A new shortened link was just created.\n\ncode: ${link.code}\nurl: ${link.url}\n\nInvestigate and decide.`,
    },
  ]

  for (let turn = 0; turn < MAX_TURNS; turn++) {
    const resp = await complete(messages)
    messages.push({ role: 'assistant', content: resp.content })

    const call = pickToolCall(resp.content)
    if (!call) {
      logger.warn('agent did not produce a tool call; ending turn', { code: link.code, stop_reason: resp.stop_reason })
      return
    }

    if (call.name === 'allow') {
      logger.info('agent: allow', { code: link.code, reason: reasonOf(call.input) })
      return
    }
    if (call.name === 'quarantine') {
      await quarantine(link.code, reasonOf(call.input))
      logger.info('agent: quarantined', { code: link.code, reason: reasonOf(call.input) })
      return
    }
    if (call.name === 'propose_delete') {
      const r = await proposeDelete(link.code)
      logger.info('agent: propose_delete', { code: link.code, reason: reasonOf(call.input), confirmed: r.confirmed })
      return
    }

    if (call.name === 'inspect_url') {
      const target = typeof call.input.url === 'string' ? call.input.url : link.url
      const result = await inspectUrl(target)
      // Feed the tool result back into the conversation as the next user turn.
      messages.push({
        role: 'user',
        content: [{ type: 'tool_result', tool_use_id: call.id, content: result }],
      })
      continue
    }

    logger.warn('agent: unknown tool', { code: link.code, name: call.name })
    return
  }
  logger.warn('agent: max turns reached without a verdict', { code: link.code })
}

// --- Subscribe to link.created (with sampling) ---

// Pubsub subscriber: samples link.created events, then enqueues an
// investigation. The queue (configured in iii-queue's queue_configs)
// gives us retries on crash, a dead-letter queue, and a concurrency
// cap so the agent never fans out faster than we can absorb.
worker.registerFunction('safety::on_link_created', async (data: { code: string; url: string }) => {
  if (Math.random() >= SAMPLE_RATE) {
    return { sampled: false }
  }
  await worker.trigger({
    function_id: 'safety::investigate',
    payload: data,
    action: TriggerAction.Enqueue({ queue: 'safety-investigations' }),
  })
  return { sampled: true, queued: true }
})

worker.registerTrigger({
  type: 'subscribe',
  function_id: 'safety::on_link_created',
  config: { topic: 'link.created' },
})

// Queue consumer: drains `safety-investigations` and runs the agent's
// tool-calling loop for one link. Throws on failure so iii-queue retries.
worker.registerFunction('safety::investigate', async (data: { code: string; url: string }) => {
  await investigate(data)
  return { investigated: true }
})

logger.info('safety-agent ready', { sample_rate: SAMPLE_RATE, stub: STUB, model: STUB ? 'stub' : MODEL })
