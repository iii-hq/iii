export interface PayoffMetric {
  label: string
  before: string
  after: string
}
export interface PayoffSolve {
  problem: string
  answer: string
  detail: string
}

export const PAYOFF_METRICS: PayoffMetric[] = [
  { label: 'types hand-written', before: 'one set per consumer', after: '0' },
  { label: 'function-id typo', before: 'runtime 404', after: 'compile error' },
  { label: 'languages from one catalog', before: 're-typed each', after: '4 generated' },
  { label: 'drift caught', before: 'in production', after: 'in ci' },
]

export const PAYOFF_SOLVES: PayoffSolve[] = [
  {
    problem: 'input / output re-typed by hand',
    answer: 'generated from the registered schema',
    detail: 'one source of truth: the engine catalog',
  },
  {
    problem: 'function id is a bare string',
    answer: 'a namespaced, typed method',
    detail: 'a typo no longer compiles',
  },
  {
    problem: 'no cross-language parity',
    answer: 'one catalog, every language',
    detail: 'typescript · rust · python · javascript',
  },
  {
    problem: 'trigger payloads are untyped',
    answer: 'typed config, payload, and return',
    detail: 'worker.on<Trigger>(config, handler) helpers',
  },
  {
    problem: 'drift is found in production',
    answer: 'codegen --check fails ci',
    detail: 'byte-deterministic output, clean diffs',
  },
]
