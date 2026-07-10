import type { FanHandler } from '@lib/components/diagrams/FanOut'

export const FAN_SOURCE = {
  label: 'harness turn loop',
  sub: 'persist · assemble · generate · run calls · repeat',
}

export const FAN_TRIGGER = 'harness::turn-completed'

export const FAN_HANDLERS: FanHandler[] = [
  {
    id: 'chat',
    label: 'chat-ui::render',
    desc: 'stream the finished turn into the conversation view',
  },
  {
    id: 'bridge',
    label: 'telegram::reply',
    desc: 'forward the answer back to the chat',
  },
  {
    id: 'metrics',
    label: 'metrics::record',
    desc: 'log tokens, cost, and latency per turn',
  },
]

export interface TriggerRow {
  id: string
  kind: string
  fires: string
}

/** the harness trigger surface a consumer can codegen against (harness/README.md) */
export const HARNESS_TRIGGERS: TriggerRow[] = [
  {
    id: 'harness::turn-started',
    kind: 'async event',
    fires: 'a turn began executing (first loop step).',
  },
  {
    id: 'harness::turn-completed',
    kind: 'async event',
    fires: 'a turn reached a terminal status, carrying the result.',
  },
  {
    id: 'harness::hook::pre-turn',
    kind: 'sync hook',
    fires: 'first step of a turn, before any model spend. may veto.',
  },
  {
    id: 'harness::hook::pre-generate',
    kind: 'sync hook',
    fires: 'after context assembly, before generation. may veto.',
  },
  {
    id: 'harness::hook::post-generate',
    kind: 'sync hook',
    fires: 'after the final assistant message. observe only.',
  },
  {
    id: 'harness::hook::pre-trigger',
    kind: 'sync hook',
    fires: 'after the allow/deny policy passes, before the call runs.',
  },
  {
    id: 'harness::hook::post-trigger',
    kind: 'sync hook',
    fires: 'after the call returns, before the result is persisted.',
  },
]
