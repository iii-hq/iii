/* namespace — the routing story as a sequence (A5). data only. */

import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

export const NS_LANES: SeqLane[] = [
  { id: 'caller', label: 'caller', x: 90 },
  { id: 'engine', label: 'engine', x: 390 },
  { id: 'orders', label: 'state · ns orders', x: 660 },
  { id: 'analytics', label: 'state · ns analytics', x: 890 },
]

export const NS_STEPS: SeqStep[] = [
  {
    from: 'orders',
    to: 'engine',
    label: 'register state',
    title: 'first instance registers',
    desc: 'compose injected III_NAMESPACE=orders; the worker code never mentions it. function ids stay exactly what it registered.',
  },
  {
    from: 'analytics',
    to: 'engine',
    label: 'register state',
    title: 'same name, second namespace',
    desc: 'the same published worker, run by another project. accepted — the namespace is a separate dimension, not a rename.',
  },
  {
    from: 'caller',
    to: 'engine',
    label: 'trigger state::get',
    title: 'no namespace given',
    desc: 'the engine picks the best fit — the caller’s own namespace first, then the default. single-instance users never notice.',
  },
  {
    from: 'engine',
    to: 'orders',
    label: 'route → ns orders',
    title: 'best fit resolves',
    desc: 'the call lands on the instance in the caller’s namespace. ids stayed two-segment the whole way.',
  },
  {
    from: 'caller',
    to: 'engine',
    label: 'trigger state::get @ analytics',
    title: 'explicit namespace',
    desc: 'trigger() gains an optional namespace argument — the call targets that instance precisely.',
  },
  {
    from: 'engine',
    to: 'analytics',
    label: 'route → ns analytics',
    title: 'targeted delivery',
    desc: 'two instances of one worker coexist on one engine, addressable without renaming a single function.',
  },
  {
    from: 'caller',
    to: 'engine',
    label: 'register state (ns orders)',
    title: 'collision — rejected',
    desc: 'a second state in the same namespace is refused at registration. today the engine warns and overwrites; this makes it a hard error.',
  },
]
