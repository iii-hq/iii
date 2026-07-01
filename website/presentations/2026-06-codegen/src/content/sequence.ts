import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

export const SEQ_LANES: SeqLane[] = [
  { id: 'dev', label: 'developer', x: 130 },
  { id: 'codegen', label: 'codegen', x: 490 },
  { id: 'engine', label: 'engine', x: 850 },
]

export const SEQ_STEPS: SeqStep[] = [
  {
    from: 'dev',
    to: 'codegen',
    label: 'codegen generate',
    title: 'run it',
    desc: 'point codegen at codegen.yml. it connects to the engine as a transient worker, then disconnects when done.',
  },
  {
    from: 'codegen',
    to: 'engine',
    label: 'engine::functions::list',
    title: 'enumerate the catalog',
    desc: 'ask the engine for every connected worker and the functions it registered.',
  },
  {
    from: 'engine',
    to: 'codegen',
    label: '{ functions: FunctionSummary[] }',
    title: 'filter by your globs',
    desc: 'names and workers come back; codegen keeps the ones your workers / functions / triggers selectors match.',
  },
  {
    from: 'codegen',
    to: 'engine',
    label: 'engine::functions::info',
    title: 'fetch each schema',
    desc: 'for every selected function, pull its request and response json schema.',
  },
  {
    from: 'engine',
    to: 'codegen',
    label: 'request_schema · response_schema',
    title: 'the schema, verbatim',
    desc: 'the exact json schema the worker derived from its real input and output types at startup. nothing approximated, nothing invented.',
  },
  {
    from: 'codegen',
    to: 'codegen',
    label: 'map + emit',
    title: 'project to types',
    desc: 'json schema becomes idiomatic types and a wrapper that lowers to a single iii.trigger call.',
  },
  {
    from: 'codegen',
    to: 'dev',
    label: 'files written',
    title: 'done',
    desc: 'deterministic files land in your repo. rerun on an unchanged catalog and the diff is empty.',
  },
]
