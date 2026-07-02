/* hero — the win, quantified. data only. */

export const HERO_STATS = [
  { value: '8/8', label: 'discovery fns filtered' },
  { value: '0', label: 'engine changes' },
  { value: '1', label: 'process = blast radius' },
  { value: 'any', label: 'engine it can front' },
] as const

export const HERO_CLAIMS = [
  {
    title: 'a separate process',
    body: 'rbac runs out-of-process, on its own port. the blast radius of an auth bug is the proxy, not the engine.',
  },
  {
    title: "the engine's exact rules",
    body: 'the decision logic is vendored from the engine, byte-for-byte. a connection is gated exactly as it would be in-engine.',
  },
  {
    title: 'in front of any engine',
    body: 'it is just a worker pointed at an engine url, so it can front a remote or managed engine you do not own.',
  },
  {
    title: 'discovery, filtered',
    body: 'all eight engine:: discovery functions are rewritten to the caller. you only ever see what you can call.',
  },
] as const
