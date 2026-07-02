import type { CliTrack } from '@lib/components/diagrams/CliPlayground'

export const CLI_TRACKS: CliTrack[] = [
  {
    id: 'generate',
    label: 'generate',
    lines: [
      { cmd: 'codegen generate --config codegen.yml', fn: 'codegen::generate' },
      {
        out: ['→ connecting to engine · ws://127.0.0.1:49134', '→ catalog · 42 functions · 9 trigger types'],
      },
      {
        out: [
          '✓ src/iii/harness.ts   ts   7 fns  2 trig  11 types  4.2kb',
          '✓ src/iii/storage.ts   ts   5 fns  0 trig   6 types  2.1kb',
        ],
      },
      { out: ['✓ done · 2 files written · 0 warnings'], exit: 0 },
    ],
  },
  {
    id: 'check',
    label: 'ci · --check',
    lines: [
      {
        cmd: 'codegen generate --config codegen.yml --check',
        fn: 'codegen::generate',
      },
      { out: ['→ catalog · 42 functions', '✓ src/iii/storage.ts   unchanged'] },
      {
        out: ['✗ src/iii/harness.ts   would change', '!  harness::send input drifted since last generate'],
      },
      { out: ['✗ check failed · 1 of 2 outputs stale · run codegen generate'], exit: 1 },
    ],
  },
]
