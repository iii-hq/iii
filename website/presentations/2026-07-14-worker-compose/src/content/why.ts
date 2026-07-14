/* why — six concrete failures in today's lifecycle, each cited. data only. */

export const WHY_CARDS = [
  {
    title: 'duplicate names overwrite silently',
    body: 'the engine warns and replaces on a name collision — a unit test codifies the overwrite. the losing process keeps running blind in the background.',
    cite: 'engine/src/services.rs:106',
  },
  {
    title: 'three ways to find the engine',
    body: 'http, state, storage and database read only a --url flag with a hardcoded default. llm-router reads III_WS_URL. shell reads III_URL. no sdk reads any of them.',
    cite: 'workers/http/src/main.rs:28',
  },
  {
    title: 'config applies once, then never',
    body: 'a worker’s --config seeds its configuration entry on first boot only; afterwards the flag is ignored. what a compose file says is not what a restarted process gets.',
    cite: 'workers/http/src/main.rs:23',
  },
  {
    title: 'startup order drops routes',
    body: 'register a route before the router worker is up and the engine prints a warning and forgets it. ordering is a correctness bug today.',
    cite: '2026-07-13 review',
  },
  {
    title: 'lifecycle is welded to one machine',
    body: 'iii worker manages processes only where the engine runs. workers cannot live where their resources are.',
    cite: 'crates/iii-worker',
  },
  {
    title: 'a dead dependency corrupts quietly',
    body: 'a queue worker keeps pushing into a crashed database. nothing stops the chain, nothing tells you.',
    cite: 'lifecycle.md',
  },
] as const
