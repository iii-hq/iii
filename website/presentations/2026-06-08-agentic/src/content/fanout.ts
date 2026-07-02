/**
 * fanout.ts — the handler/edge data ReactiveSection feeds the shared
 * EventFanOut archetype: one session write reaching every bound surface.
 */

import type { FanOutEdge, FanOutHandler } from '@lib/components/diagrams/EventFanOut'

export const FANOUT_HANDLERS: FanOutHandler[] = [
  { id: 'chat::render', sub: 'console ui — deltas in place', y: 30 },
  { id: 'tg::on-message-updated', sub: 'telegram — edit the message', y: 108 },
  { id: 'metrics::on_update', sub: 'dashboard — usage tiles', y: 186 },
]

export const FANOUT_EDGES: FanOutEdge[] = [
  { d: 'M 612 132 C 666 132, 676 52, 728 52', dur: 1.8 },
  { d: 'M 612 132 L 728 130', dur: 1.5 },
  { d: 'M 612 132 C 666 132, 676 208, 728 208', dur: 2.1 },
]
