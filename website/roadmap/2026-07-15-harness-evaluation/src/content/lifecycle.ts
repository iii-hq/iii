/* lifecycle — the ten-stage scenario walk (A6) + failure classifications. */
import type { RevealStage } from '@lib/components/diagrams/StepReveal'

export const LIFECYCLE_STAGES: RevealStage[] = [
  {
    label: 'allocate',
    tone: 'ink',
    caption: 'run id, working directory, isolated stores, reserved ports, deadlines, artifact directory. every identity is scoped by run_id.',
    rows: [
      { k: 'run_id', v: 'r_9f2c…' },
      { k: 'stack', v: 'fresh, never reused' },
    ],
  },
  {
    label: 'boot',
    tone: 'ink',
    caption: 'pinned engine, real durable dependencies, scripted router, recorder, and the harness artifact under test, started in declared order with captured stdout/stderr.',
    rows: [
      { k: 'engine', v: 'pinned digest, recorded' },
      { k: 'secrets', v: 'env allowlist, no inherited keys' },
    ],
  },
  {
    label: 'probe',
    tone: 'ink',
    caption: 'readiness is schema-based, never sleep-based: exact functions with compatible schemas, the turn-completed trigger type, the harness-turn queue topic.',
    rows: [
      { k: 'check', v: 'schemas · triggers · topics' },
      { k: 'on miss', v: 'setup_error, names every surface' },
    ],
  },
  {
    label: 'arm',
    tone: 'ink',
    caption: 'load the router script, configure and verify the run-scoped recorder target, reset the durable log, create the lifecycle binding. no send until discovery shows the target and the log reads empty.',
    rows: [
      { k: 'script', v: 'validated before the stack' },
      { k: 'recorder', v: 'empty, sequence = 1' },
    ],
  },
  {
    label: 'send',
    tone: 'ink',
    caption: 'one ordinary harness::send with a run-scoped idempotency key. the exact request and response become evidence.',
    rows: [
      { k: 'entry', v: 'harness::send, public only' },
      { k: 'recorded', v: 'request + response, verbatim' },
    ],
  },
  {
    label: 'await',
    tone: 'ink',
    caption: 'key on both the returned session and turn id. identical duplicate events are accepted; terminal durable status is confirmed independently.',
    rows: [
      { k: 'duplicates', v: 'accepted if identical' },
      { k: 'conflicts', v: 'terminal disagreement fails' },
    ],
  },
  {
    label: 'collect',
    tone: 'ink',
    caption: 'paginate the transcript to the last cursor; snapshot router, target, lifecycle, and process evidence.',
    rows: [
      { k: 'transcript', v: 'all pages, ordered' },
      { k: 'recorder', v: 'strictly increasing sequence' },
    ],
  },
  {
    label: 'grade',
    tone: 'warn',
    caption: 'pure code assertions over the evidence: no mutation of the subject, no single oracle sufficient, private state never decides.',
    rows: [
      { k: 'oracle', v: 'code invariants only' },
      { k: 'expected vs actual', v: 'kept per invariant' },
    ],
  },
  {
    label: 'report',
    tone: 'ink',
    caption: 'canonical json plus concise console output. deterministic report bytes are an acceptance requirement.',
    rows: [
      { k: 'result.json', v: 'classification + invariants' },
      { k: 'bytes', v: 'deterministic' },
    ],
  },
  {
    label: 'teardown',
    tone: 'accent',
    caption: 'sigterm, five seconds, sigkill. artifacts are retained according to classification; a pass may discard verbose evidence.',
    rows: [
      { k: 'children', v: '0 remaining' },
      { k: 'retention', v: 'by classification' },
    ],
  },
]

export const CLASSIFICATIONS = [
  {
    id: 'pass',
    exit: '0',
    tone: 'accent' as const,
    desc: 'every required invariant passed. the only way to exit 0.',
  },
  {
    id: 'contract_failure',
    exit: '2',
    tone: 'alert' as const,
    desc: 'the subject broke a public contract: matcher failure, unexpected call, bad transcript.',
  },
  {
    id: 'timeout',
    exit: '2',
    tone: 'alert' as const,
    desc: 'the subject exceeded the scenario deadline after send.',
  },
  {
    id: 'setup_error',
    exit: '3',
    tone: 'warn' as const,
    desc: 'missing infrastructure before send. never a skip, never a pass.',
  },
  {
    id: 'process_crash',
    exit: '3',
    tone: 'warn' as const,
    desc: 'a stack process exited unexpectedly; classified before any ordinary timeout.',
  },
  {
    id: 'runner_error',
    exit: '3',
    tone: 'warn' as const,
    desc: 'the runner itself broke; its deadlines and bugs are owned separately from the subject’s.',
  },
] as const
