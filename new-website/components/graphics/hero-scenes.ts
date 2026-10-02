import type { BundledLanguage } from 'shiki'

/** The three hero scenes: register a function, call it from Python, then trace a queued call. */
export const heroScenes = [
  {
    label: 'Register',
    file: 'reports.ts',
    language: 'TypeScript',
    lang: 'ts' as BundledLanguage,
    code: [
      'iii.registerFunction(',
      "  'reports::generate',",
      '  async ({ team }:',
      '    { team: string }) => {',
      '    return buildReport(team)',
      '  }',
      ')',
    ],
    worker: 'node',
  },
  {
    label: 'Call',
    file: 'billing.py',
    language: 'Python',
    lang: 'python' as BundledLanguage,
    code: [
      'report = iii.trigger({',
      '  "function_id":',
      '    "reports::generate",',
      '  "payload": {',
      '    "team": "billing"',
      '  }',
      '})',
    ],
    worker: 'python',
  },
  {
    label: 'Trace',
    file: 'reports.ts',
    language: 'TypeScript',
    lang: 'ts' as BundledLanguage,
    code: [
      'await iii.trigger({',
      '  function_id:',
      "    'reports::generate',",
      '  payload: {',
      "    team: 'billing' },",
      '  action:',
      '    TriggerAction.Enqueue({',
      "      queue: 'reports'",
      '    }),',
      '})',
    ],
    worker: 'queue',
  },
] as const
