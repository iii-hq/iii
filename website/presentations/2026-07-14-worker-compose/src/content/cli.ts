/* cli contract — today's divergence vs the standard. data only. */

export const CLI_TODAY = [
  { worker: 'http · state · storage · database · harness', reads: '--url flag only', env: 'none', default: 'ws://127.0.0.1:49134 hardcoded' },
  { worker: 'llm-router', reads: '--url flag', env: 'III_WS_URL', default: 'ws://127.0.0.1:49134' },
  { worker: 'shell · codex · devin · email · bridge · lsp', reads: 'varies', env: 'III_URL', default: 'varies' },
  { worker: 'every sdk (register_worker)', reads: 'explicit code argument', env: 'none', default: '—' },
] as const

export const CLI_STANDARD = [
  { param: 'engine address', flag: '--url', env: 'III_URL' },
  { param: 'namespace', flag: '--namespace', env: 'III_NAMESPACE' },
  { param: 'configuration', flag: '--config', env: 'III_CONFIG' },
] as const

export const CLI_NOTES = [
  'flags win over env; env wins over defaults.',
  'registerWorker() with no arguments reads the env contract — tutorial code carries no addresses and no namespaces.',
  'the daemon injects all three into every child and every hook; a compose file cannot override them.',
  'one shared config-registration library gives the whole fleet the contract in a dependency bump — not 41 hand-rolled parsers.',
] as const
