/* why — the pain of engine-native rbac. data only. */

export interface PainCard {
  n: string
  title: string
  body: string
}

/**
 * today's failures, grounded in README § "why this exists" and
 * engine-overrides.md § the discovery leak.
 */
export const PAIN_CARDS: PainCard[] = [
  {
    n: '01',
    title: 'rbac is welded to the engine',
    body: 'the same rbac surface ships inside the engine, configured from engine/compose config. it can only guard an engine you operate.',
  },
  {
    n: '02',
    title: "can't front an engine you don't own",
    body: 'a hosted iii, a teammate’s engine, a managed deployment: there is no way to put your own access policy in front of an engine that is not yours.',
  },
  {
    n: '03',
    title: 'one auth bug, whole-engine blast radius',
    body: 'when rbac runs in the engine process, a bug in the policy path is a bug in the engine. you cannot deploy, scale, or harden it on its own.',
  },
  {
    n: '04',
    title: 'discovery leaks what you cannot call',
    body: 'only 2 of the 8 engine:: discovery functions are session-aware, and only in-process. a gated worker can still enumerate functions, workers, and triggers it can never invoke.',
  },
]

/** the eight discovery functions and whether the engine filters them per-session. */
export interface DiscoveryRow {
  fn: string
  sessionAware: boolean
  leaks: string
}

export const DISCOVERY_LEAK: DiscoveryRow[] = [
  { fn: 'engine::functions::list', sessionAware: true, leaks: 'in-process only; a remote proxy gets no session' },
  { fn: 'engine::functions::info', sessionAware: true, leaks: 'in-process only; schemas, metadata' },
  { fn: 'engine::workers::list', sessionAware: false, leaks: 'every worker, pid, ip, function_count' },
  { fn: 'engine::workers::info', sessionAware: false, leaks: 'per-worker functions, triggers, metrics' },
  { fn: 'engine::triggers::list', sessionAware: false, leaks: 'every trigger type' },
  { fn: 'engine::triggers::info', sessionAware: false, leaks: 'instance_count over hidden functions' },
  { fn: 'engine::registered-triggers::list', sessionAware: false, leaks: 'bindings to functions you cannot call' },
  { fn: 'engine::registered-triggers::info', sessionAware: false, leaks: 'nested function + trigger envelopes' },
]
