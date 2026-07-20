/* lifecycle — the twelve-stage scenario walk (A6) + failure classifications. */
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
    label: 'boot base',
    tone: 'ink',
    caption: 'pinned engine, real durable dependencies, scripted router, and the recorder, started in declared order with captured stdout/stderr. the harness does not boot yet.',
    rows: [
      { k: 'engine', v: 'pinned digest, recorded' },
      { k: 'secrets', v: 'env allowlist, no inherited keys' },
    ],
  },
  {
    label: 'probe base',
    tone: 'ink',
    caption: 'readiness is contract-based, never sleep-based: live descriptors through engine::functions::info, canonically compared against checked-in goldens, plus the configuration seed, triggers, and queue topic.',
    rows: [
      { k: 'check', v: 'live descriptors vs goldens' },
      { k: 'on miss', v: 'setup_error + readiness-failure.json' },
    ],
  },
  {
    label: 'arm target',
    tone: 'ink',
    caption: 'load the router script, reset the durable log, register the run-scoped target. the runner then verifies the live descriptor independently: exact description, canonical request schema, and the compiler-derived response schema.',
    rows: [
      { k: 'script', v: 'validated before the stack' },
      { k: 'recorder', v: 'empty, sequence = 1' },
    ],
  },
  {
    label: 'boot harness',
    tone: 'ink',
    caption: 'only now does the harness under test start; its live contracts are probed the same way. native discovery can snapshot registrations mid-boot, so the target is armed and verified first.',
    rows: [
      { k: 'order', v: 'target armed → harness boots' },
      { k: 'probe', v: 'live harness descriptors' },
    ],
  },
  {
    label: 'bind',
    tone: 'ink',
    caption: 'install and verify the lifecycle binding last, after both the base stack and harness surfaces have passed readiness.',
    rows: [
      { k: 'trigger', v: 'harness::turn-completed' },
      { k: 'sink', v: 'integration-recorder::lifecycle' },
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
    caption: 'key on both the returned session and turn id. identical duplicate events are accepted; terminal durable status is confirmed independently. a deadline missed here is the only subject timeout.',
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
    label: 'teardown',
    tone: 'ink',
    caption: 'sigterm, five seconds, sigkill, then a typed teardown report. remaining process groups, signal errors, or a cleanup deadline produce runner_error; a warning-only incomplete teardown cannot leave a scenario green.',
    rows: [
      { k: 'children', v: '0 remaining, typed report' },
      { k: 'teardown.json', v: 'always written' },
    ],
  },
  {
    label: 'report',
    tone: 'accent',
    caption: 'two files: a stable, byte-comparable result.json and a volatile execution.json carrying run identity and timing, linked by the sha-256 of the exact result bytes.',
    rows: [
      { k: 'result.json', v: 'stable, canonical verdict' },
      { k: 'execution.json', v: 'sha-256 linked' },
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
    desc: 'the subject exceeded its completion deadline while the runner was in await.',
  },
  {
    id: 'setup_error',
    exit: '3',
    tone: 'warn' as const,
    desc: 'missing infrastructure or a readiness mismatch before send. never a skip, never a pass.',
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
    desc: 'recorder, collection, artifact, or teardown failure after send. infrastructure deadlines never become subject timeouts.',
  },
] as const
