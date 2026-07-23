/* ordering — the optimistic registration buffer as a sequence (A5). data only. */

import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

export const ORDER_LANES: SeqLane[] = [
  { id: 'api', label: 'api worker', x: 120 },
  { id: 'engine', label: 'engine', x: 460 },
  { id: 'router', label: 'http router', x: 820 },
]

export const ORDER_STEPS: SeqStep[] = [
  {
    from: 'api',
    to: 'engine',
    label: 'register route (http trigger)',
    title: 'the route arrives early',
    desc: 'the api worker registers an http route, but the router worker is not up yet. today: a warning, and the route is dropped.',
  },
  {
    from: 'engine',
    to: 'engine',
    label: 'hold in buffer',
    title: 'optimistic, not lossy',
    desc: 'the engine keeps the registration in an in-memory buffer with a bounded ttl — compose exposes the timeout and retry knobs.',
  },
  {
    from: 'router',
    to: 'engine',
    label: 'router registers',
    title: 'the trigger owner arrives',
    desc: 'the http router worker comes up (first boot or a restart) and registers its trigger type.',
  },
  {
    from: 'engine',
    to: 'router',
    label: 'flush buffer → route live',
    title: 'the buffer flushes',
    desc: 'every held registration replays automatically. no worker had to know about the ordering.',
  },
  {
    from: 'engine',
    to: 'api',
    label: 'route confirmed',
    title: 'order is now a performance concern',
    desc: 'depends_on stays about data dependencies; registration order stops being a correctness bug.',
  },
]
