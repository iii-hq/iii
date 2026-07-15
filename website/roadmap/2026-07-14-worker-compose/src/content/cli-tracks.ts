/* run-it — the golden path and its three edges, as playable cli tracks. */

import type { CliTrack } from '@lib/components/diagrams/CliPlayground'

export const CLI_TRACKS: CliTrack[] = [
  {
    id: 'up',
    label: 'up',
    lines: [
      {
        cmd: 'iii compose --id host-a --file worker-compose.yaml',
        out: ['→ waiting for engine … connected', '✓ registered compose::up · down · list · status · logs · validate'],
      },
      {
        cmd: 'iii trigger compose::up id=host-a',
        fn: 'compose::up',
        out: [
          '→ config orders-database fetched · override applied',
          '[pre_start] database: (none)',
          '✓ database ready · ns orders',
          '[pre_start] api: npx prisma migrate deploy … done',
          '✓ api ready · ns orders',
          '{ "status": "success", "changed": true }',
        ],
        exit: 0,
      },
    ],
  },
  {
    id: 'again',
    label: 'up, again',
    lines: [
      {
        cmd: 'iii trigger compose::up id=host-a',
        fn: 'compose::up',
        out: ['→ database already running · api already running', '{ "status": "success", "changed": false }'],
        exit: 0,
      },
    ],
  },
  {
    id: 'collision',
    label: 'collision',
    lines: [
      {
        cmd: 'iii compose --id host-b --file worker-compose.yaml   # same file, same namespace',
        out: ['→ waiting for engine … connected'],
      },
      {
        cmd: 'iii trigger compose::up id=host-b',
        fn: 'compose::up',
        out: [
          '✗ rejected: database already registered in namespace orders (owner: host-a)',
          '→ no silent overwrite. the second process exits instead of running blind.',
        ],
        exit: 1,
      },
    ],
  },
  {
    id: 'down',
    label: 'down',
    lines: [
      {
        cmd: 'iii trigger compose::down id=host-a',
        fn: 'compose::down',
        out: [
          '→ stopping api (dependents first)',
          '[post_run] api: ./cleanup-tmp.sh fired (not awaited)',
          '→ stopping database',
          '[post_run] database: ./backup-on-exit.sh fired',
          '✓ 0 processes left behind',
          '{ "status": "success", "changed": true }',
        ],
        exit: 0,
      },
    ],
  },
]
