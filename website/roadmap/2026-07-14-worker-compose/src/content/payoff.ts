/* payoff — problem → answer, plus the honest costs. data only. */

export const SCORECARD = [
  { problem: 'duplicate names overwrite silently', answer: 'rejected registration inside a namespace — fail early, no zombies' },
  { problem: 'three conventions to find the engine', answer: 'one cli/env contract: --url · --namespace · --config, zero-arg registerWorker()' },
  { problem: 'config applies on first boot only', answer: 'daemon fetches base, merges overrides, delivers the final config at start' },
  { problem: 'startup order drops routes', answer: 'optimistic buffer with bounded ttl — order becomes performance, not correctness' },
  { problem: 'a dead dependency corrupts quietly', answer: 'cascading stop in reverse dependency order, cause in the logs' },
  { problem: 'lifecycle welded to the engine host', answer: 'one daemon per machine, all attached to one engine' },
] as const

export const PAYOFF_TRADEOFFS_TITLE = 'the honest costs'

export const TRADEOFFS = [
  { title: 'the namespace surface is wide', body: 'register + trigger protocol messages, engine routing, and all three sdks change together. it is the largest single piece and it is priced in, not hidden.' },
  { title: 'scripts return to the compose file', body: 'the 2026-07-13 review removed them; this pack brings back a scoped version (three hooks, run only for local workers). needs explicit re-approval.' },
  { title: 'artifacts are per-target', body: 'a real release ships 9 platform binaries with 9 digests — reproducibility metadata is per-target from day one or it is wrong.' },
] as const
