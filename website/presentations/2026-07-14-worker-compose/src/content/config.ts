/* config — three sources converge on one started process (A8). data only. */

import type { FunnelPath } from '@lib/components/diagrams/Funnel'

export const CONFIG_PATHS: FunnelPath[] = [
  { id: 'defaults', label: 'worker defaults', desc: 'shipped with the package' },
  { id: 'base', label: 'base config', desc: 'configuration worker · by name' },
  { id: 'override', label: 'config_override', desc: 'sparse map in the compose file' },
]

export const CONFIG_TARGET = {
  label: 'the process starts configured',
  sub: 'daemon merges, then spawns — no startup dance',
}

export const CONFIG_REJECT = {
  label: 'full config in yaml',
  desc: 'two sources of truth — rejected',
}

export const CONFIG_RULES = [
  { name: 'reference by name', desc: 'config_name resolves through the configuration worker; storage is an adapter (fs, secrets manager, another iii). file paths are not the contract.' },
  { name: 'fetch or fail', desc: 'a container whose base fetch fails does not start — up fails before a process exists rather than booting on wrong defaults.' },
  { name: 'merge semantics', desc: 'maps merge per key; arrays and scalars replace; null is an explicit value.' },
  { name: 'why values, not a pointer', desc: '--config is a first-boot seed today ("thereafter the configuration worker is the authoritative source"). delivering resolved values keeps the compose file honest on every restart.' },
] as const
