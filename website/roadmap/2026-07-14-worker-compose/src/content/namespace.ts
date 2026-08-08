/* namespace — the routing story as a sequence (A5). data only. */

import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

export const NS_LANES: SeqLane[] = [
  { id: 'caller', label: 'caller', x: 90 },
  { id: 'engine', label: 'engine', x: 390 },
  { id: 'default', label: 'state · ns default', x: 660 },
  { id: 'analytics', label: 'state · ns analytics', x: 890 },
]

export const NS_STEPS: SeqStep[] = [
  {
    from: 'default',
    to: 'engine',
    label: 'register state',
    title: 'first instance registers',
    desc: 'started by hand, no env — it lands in the default namespace. function ids stay exactly what it registered.',
  },
  {
    from: 'analytics',
    to: 'engine',
    label: 'register state',
    title: 'same name, second namespace',
    desc: 'the same published worker with III_NAMESPACE=analytics injected. accepted — the namespace is a separate dimension, not a rename.',
  },
  {
    from: 'caller',
    to: 'engine',
    label: 'trigger("state::get")',
    title: 'no namespace argument',
    desc: 'strict rule: no namespace means the default namespace, and only it. no best-fit guessing.',
  },
  {
    from: 'engine',
    to: 'default',
    label: 'route → ns default',
    title: 'default resolves',
    desc: 'the call lands on the default-namespace instance. a miss would be a clear not-found listing where the id does exist.',
  },
  {
    from: 'caller',
    to: 'engine',
    label: 'trigger("state::get", namespace: "analytics")',
    title: 'explicit namespace',
    desc: 'the namespace argument targets that instance precisely — the only way a namespaced instance is reached.',
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
    label: 'register state (ns analytics)',
    title: 'collision — rejected',
    desc: 'a second state in the same namespace is refused at registration. today the engine warns and overwrites; this makes it a hard error.',
  },
]
