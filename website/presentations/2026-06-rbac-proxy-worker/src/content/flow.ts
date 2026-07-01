/* flow — the request lifecycle (A5) and the engine:: override walk (A6). */
import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'
import type { RevealStage } from '@lib/components/diagrams/StepReveal'

/* ---- request lifecycle: every frame, intercepted (A5) ---- */

export const SEQ_LANES: SeqLane[] = [
  { id: 'worker', label: 'worker', x: 120 },
  { id: 'proxy', label: 'rbac-proxy', x: 380 },
  { id: 'auth', label: 'auth fn', x: 620 },
  { id: 'engine', label: 'engine', x: 860 },
]

export const SEQ_STEPS: SeqStep[] = [
  {
    from: 'worker',
    to: 'proxy',
    label: 'WS upgrade',
    title: 'connect',
    desc: "the worker opens a websocket to the proxy's public port, carrying headers, query params, and its ip.",
  },
  {
    from: 'proxy',
    to: 'auth',
    label: 'iii.trigger(auth)',
    title: 'authenticate, once',
    desc: "the proxy calls the operator's auth function on its control connection, with an authinput built from the upgrade request.",
  },
  {
    from: 'proxy',
    to: 'proxy',
    label: 'build session',
    title: 'derive boundaries',
    desc: 'from the authresult the proxy builds a per-connection session: allowed, forbidden, expose, prefix, context, and trigger permissions.',
  },
  {
    from: 'proxy',
    to: 'engine',
    label: 'open upstream ws',
    title: 'one upstream per connection',
    desc: 'only after auth passes does the proxy dial a fresh outbound websocket to the trusted engine listener.',
  },
  {
    from: 'engine',
    to: 'worker',
    label: 'WorkerRegistered',
    title: 'registered',
    desc: "the engine's first frame carries the worker_id. the proxy forwards it downstream unchanged.",
  },
  {
    from: 'worker',
    to: 'proxy',
    label: 'InvokeFunction',
    title: 'a call arrives',
    desc: 'every frame the worker sends hits the interceptor before anything else.',
  },
  {
    from: 'proxy',
    to: 'proxy',
    label: 'intercept',
    title: 'gate · prefix · hook · middleware · override',
    desc: 'the proxy runs access resolution, applies the namespace prefix, runs registration hooks and middleware, and marks engine:: discovery calls for rewriting.',
  },
  {
    from: 'proxy',
    to: 'engine',
    label: 'forward / deny',
    title: 'forward or synthesize',
    desc: 'an allowed call is forwarded with resolved ids. a denied call never reaches the engine: the proxy synthesizes a FORBIDDEN result itself.',
  },
  {
    from: 'engine',
    to: 'proxy',
    label: 'InvocationResult',
    title: 'result returns',
    desc: 'the engine answers. a discovery result carries more than this caller is allowed to see.',
  },
  {
    from: 'proxy',
    to: 'worker',
    label: 'rewrite + forward',
    title: 'filtered to the caller',
    desc: "the proxy strips its prefix and filters engine:: discovery results to the caller's boundaries, then forwards.",
  },
]

/* ---- engine:: discovery override walk (A6) ---- */

export const OVERRIDE_STAGES: RevealStage[] = [
  {
    label: 'invoke',
    tone: 'ink',
    caption: 'a gated worker that can reach a discovery function calls engine::workers::list.',
    rows: [
      { k: 'request', v: 'engine::workers::list' },
      { k: 'invocation', v: 'a1b2c3' },
    ],
  },
  {
    label: 'record',
    tone: 'ink',
    caption: 'the proxy records the invocation_id in its pending-override map and forwards the request unchanged.',
    rows: [
      { k: 'override', v: 'a1b2c3 → workers::list' },
      { k: 'forwarded', v: 'as-is' },
    ],
  },
  {
    label: 'engine computes',
    tone: 'alert',
    caption:
      "the engine answers with the full result. it filtered against the proxy's own session, which is unrestricted on the internal listener.",
    rows: [
      { k: 'workers', v: '12' },
      { k: 'fields', v: 'pid · ip · schemas · owners' },
    ],
    note: 'this is the full surface, more than the caller may see',
  },
  {
    label: 'filter',
    tone: 'warn',
    caption:
      'the proxy runs the same vendored is_function_allowed over every function id in the result: drop zero-access workers, recompute counts, strip the prefix and worker internals.',
    rows: [
      { k: 'basis', v: 'is_function_allowed' },
      { k: 'workers', v: '3' },
    ],
  },
  {
    label: 'downstream',
    tone: 'accent',
    caption:
      'the worker receives a result showing exactly the surface it can invoke. discovery and invocation can never disagree.',
    rows: [
      { k: 'workers', v: '3' },
      { k: 'leaked', v: 'none' },
    ],
    note: 'all eight discovery functions, rewritten client-side',
  },
]

/* ---- the per-function rewrite table (engine-overrides.md) ---- */

export interface OverrideRow {
  fn: string
  rewrite: string
  policy: string
}

export const OVERRIDE_TABLE: OverrideRow[] = [
  {
    fn: 'engine::functions::list',
    rewrite: 'keep entries where A(function_id); strip prefix from ids + worker names',
    policy: 'possibly-empty filtered list',
  },
  {
    fn: 'engine::functions::info',
    rewrite: 'error if !A(function_id); strip ids; drop registered_triggers whose target fails A',
    policy: 'FORBIDDEN on denied · NOT_FOUND on missing',
  },
  {
    fn: 'engine::triggers::list',
    rewrite: 'capability metadata, pass through; optional hardening hides types serving only denied fns',
    policy: 'unchanged (or hardened)',
  },
  {
    fn: 'engine::triggers::info',
    rewrite: 'recompute instance_count to the accessible subset via the binding index; keep schemas',
    policy: 'visible by default',
  },
  {
    fn: 'engine::registered-triggers::list',
    rewrite: 'drop entries where !A(function_id); strip function_id prefix',
    policy: 'filtered list',
  },
  {
    fn: 'engine::registered-triggers::info',
    rewrite: 'deny if !A(function_id); null out nested function/trigger envelopes for denied fns',
    policy: 'FORBIDDEN on denied',
  },
  {
    fn: 'engine::workers::list',
    rewrite:
      'resolve each worker via the catalog cache; drop zero-access workers; recompute function_count; strip internals',
    policy: 'filtered list',
  },
  {
    fn: 'engine::workers::info',
    rewrite: 'filter nested functions / trigger_types / registered_triggers to accessible; strip envelope internals',
    policy: 'NOT_FOUND when zero accessible',
  },
]
