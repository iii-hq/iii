/* hero — the win, quantified. data only. */

export const HERO_STATS = [
  { value: '41→1', label: 'binaries on one cli contract' },
  { value: '2× up', label: 'one deterministic error' },
  { value: '3', label: 'hooks, one supervised run' },
  { value: '0', label: 'function ids renamed' },
] as const

export const HERO_CLAIMS = [
  {
    title: 'a daemon per machine',
    body: 'iii compose starts a supervisor, not an engine. it owns exactly the processes it spawned; several daemons attach to one engine.',
  },
  {
    title: 'namespace, not rename',
    body: 'two projects run the same state worker side by side. function ids never change; the namespace travels as an argument.',
  },
  {
    title: 'config arrives finished',
    body: 'the daemon fetches the base, applies the file overrides, and hands the worker its final config at start. no startup dance.',
  },
  {
    title: 'failure is loud and ordered',
    body: 'a name collision is a rejected connection. a crashed dependency stops its dependents. nothing keeps running blind.',
  },
] as const
