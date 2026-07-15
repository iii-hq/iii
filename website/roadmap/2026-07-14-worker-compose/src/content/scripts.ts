/* scripts — the container lifecycle as a step reveal (A6). data only. */

import type { RevealStage } from '@lib/components/diagrams/StepReveal'

export const SCRIPT_STAGES: RevealStage[] = [
  {
    label: 'resolve config',
    caption: 'base fetched from the configuration worker, config_override merged. failure here means the container never starts.',
    rows: [
      { k: 'base', v: 'orders-api · fetched' },
      { k: 'override', v: 'server.port: 3000 · applied' },
      { k: 'process', v: 'none yet' },
    ],
  },
  {
    label: 'pre_start',
    caption: 'npx prisma migrate deploy — blocking, in its own process group, 60s default budget (pre_start_timeout to change).',
    rows: [
      { k: 'hook', v: 'running · blocking' },
      { k: 'budget', v: '60s default' },
      { k: 'on failure', v: 'container failed · no spawn · rollback' },
    ],
  },
  {
    label: 'spawn run',
    caption: 'path:// worker: the compose run command. package:// binary: exec the artifact with --url --namespace --config.',
    rows: [
      { k: 'pid', v: '48213 · own process group' },
      { k: 'injected', v: 'III_URL · III_NAMESPACE · III_CONFIG' },
      { k: 'status', v: 'starting' },
    ],
  },
  {
    label: 'ready',
    tone: 'accent',
    caption: 'readiness is the engine seeing the registration — not a live pid. dependents may start now.',
    rows: [
      { k: 'registered', v: 'api::* · ns orders' },
      { k: 'status', v: 'ready' },
      { k: 'dependents', v: 'unblocked' },
    ],
  },
  {
    label: 'run exits',
    tone: 'warn',
    caption: 'any exit path — down, a post-ready crash, or an up rollback. exit is confirmed before anything else happens.',
    rows: [
      { k: 'status', v: 'exited' },
      { k: 'cause', v: 'down · crash · rollback' },
      { k: 'registration', v: 'released after confirmed exit' },
    ],
  },
  {
    label: 'post_run',
    caption: './cleanup-tmp.sh — fired once after the exit is confirmed, never awaited. a non-zero exit is a warning, not a blocker.',
    rows: [
      { k: 'hook', v: 'fired · not awaited' },
      { k: 'on failure', v: 'warning + last_error' },
      { k: 'teardown', v: 'proceeds without waiting' },
    ],
  },
]

/* the schema behind the claim — rendered in a SpecSheet. */
export const SCRIPT_FIELDS = [
  { name: 'pre_start', desc: 'blocking; runs before every spawn; exit ≠ 0 or timeout fails the container.' },
  { name: 'pre_start_timeout', desc: 'default 60s; set the field only to change it; timeout kills the hook’s process group.' },
  { name: 'run', desc: 'the supervised process. path:// only — required when the worker has no manifest. compose run wins over manifest scripts.start.' },
  { name: 'post_run', desc: 'fired once after the run process exits — any exit path (down, crash, rollback); not awaited; failure = warning + last_error.' },
] as const

export const SCRIPT_RULES = [
  'run on a package:// worker → validation error (binaries start implicitly).',
  'path:// with neither manifest nor run → error: add run: or create iii.worker.yaml.',
  'setup/install from the manifest are never executed by compose — sandbox contract.',
  'no stop hook in v1: teardown is sigterm → grace → sigkill; post_run covers after-exit.',
] as const
