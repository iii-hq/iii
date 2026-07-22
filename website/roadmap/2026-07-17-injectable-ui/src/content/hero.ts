/* hero — stat strip + the four mechanism claims under the fold. */

export const HERO_STATS = [
  { value: '0', label: 'engine changes required' },
  { value: '26 → 0', label: 'console files per new page' },
  { value: '13 → 1', label: 'renderer chain → registry' },
  { value: '4', label: 'injection slots' },
] as const

export const HERO_CLAIMS = [
  {
    title: 'three trigger types',
    body: 'console:script and console:style carry assets from workers; console:assets is how tabs subscribe to updates. registration of any of them is forwarded to the console live by the engine — forwarded, parked, replayed. registration is the event.',
  },
  {
    title: 'content over the bus',
    body: 'trigger config carries {path} only. the console invokes the trigger’s function_id to fetch source, caches the bytes, and serves them from its own http port (:3113).',
  },
  {
    title: 'hash-keyed hot reload',
    body: 'assets version by sha256(content), first 16 hex chars. a changed hash is pushed to every subscribed tab; each disposes the old module and re-imports ?v=<hash>. no counters, no stream worker, restart-proof.',
  },
  {
    title: 'one react, scoped css',
    body: 'a static import map resolves react to the spa’s own bundled instance via /vendor shims. worker css compiles with every selector under [data-iii-ui="<worker>"] — nothing leaks either way.',
  },
] as const
