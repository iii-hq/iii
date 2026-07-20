/* sabotage — data for the break-the-run toy (deck-local). every mode maps to
   a real classification and oracle from the spec; nothing here is invented. */

export type OracleId =
  | 'send'
  | 'transcript'
  | 'status'
  | 'target'
  | 'router'
  | 'lifecycle'
  | 'supervisor'

export const ORACLE_LABELS: Array<{ id: OracleId; label: string }> = [
  { id: 'send', label: 'send response' },
  { id: 'transcript', label: 'full transcript' },
  { id: 'status', label: 'status' },
  { id: 'target', label: 'target recorder' },
  { id: 'router', label: 'scripted router' },
  { id: 'lifecycle', label: 'lifecycle recorder' },
  { id: 'supervisor', label: 'process supervisor' },
]

export type Classification =
  | 'pass'
  | 'contract_failure'
  | 'timeout'
  | 'setup_error'
  | 'process_crash'
  | 'runner_error'

export const ALL_CLASSIFICATIONS: Classification[] = [
  'pass',
  'contract_failure',
  'timeout',
  'setup_error',
  'process_crash',
  'runner_error',
]

export interface OracleCall {
  v: 'ok' | 'fail' | 'skip'
  note?: string
}

export interface SabotageMode {
  id: string
  chip: string
  deed: string
  log: Array<{ text: string; tone: 'ok' | 'bad' | 'warn' | 'dim' }>
  oracles: Record<OracleId, OracleCall>
  classification: Classification
  exit: '0' | '2' | '3'
  tone: 'accent' | 'alert' | 'warn'
  caughtBy: string
  lesson: string
}

const ALL_OK: Record<OracleId, OracleCall> = {
  send: { v: 'ok' },
  transcript: { v: 'ok' },
  status: { v: 'ok' },
  target: { v: 'ok' },
  router: { v: 'ok' },
  lifecycle: { v: 'ok' },
  supervisor: { v: 'ok' },
}

const NOT_REACHED: Record<OracleId, OracleCall> = {
  send: { v: 'skip' },
  transcript: { v: 'skip' },
  status: { v: 'skip' },
  target: { v: 'skip' },
  router: { v: 'skip' },
  lifecycle: { v: 'skip' },
  supervisor: { v: 'skip' },
}

export const SABOTAGE_MODES: SabotageMode[] = [
  {
    id: 'none',
    chip: 'play it honest',
    deed: 'run C-E2E-001 exactly as scripted.',
    log: [
      { text: 'probe: live descriptors match the checked-in goldens', tone: 'ok' },
      { text: 'harness::send accepted · session + turn recorded', tone: 'ok' },
      { text: 'frames: start · text ×2 · usage · stop · done', tone: 'dim' },
      { text: 'harness::turn-completed · status completed', tone: 'ok' },
      { text: 'transcript: one user, one assistant, "fixture complete"', tone: 'ok' },
      { text: 'result.json written · execution.json linked by sha-256', tone: 'ok' },
    ],
    oracles: ALL_OK,
    classification: 'pass',
    exit: '0',
    tone: 'accent',
    caughtBy: 'nothing. every required invariant passed.',
    lesson: 'the only way to exit 0 is to play every public contract straight.',
  },
  {
    id: 'dup-entry',
    chip: 'duplicate an entry',
    deed: 'persist the streamed partial as a second assistant message.',
    log: [
      { text: 'probe ✓ · send accepted · frames stream to done', tone: 'ok' },
      { text: 'a partial update is persisted as its own entry', tone: 'bad' },
      { text: 'status: completed, looks terminal and clean', tone: 'dim' },
      { text: 'transcript paginated: user, assistant, assistant', tone: 'bad' },
    ],
    oracles: {
      ...ALL_OK,
      transcript: { v: 'fail', note: 'duplicate assistant entry' },
    },
    classification: 'contract_failure',
    exit: '2',
    tone: 'alert',
    caughtBy: 'the full transcript, paginated to the last cursor',
    lesson: 'streaming may update in place, but durability is graded on persisted order: a partial can never become a second entry.',
  },
  {
    id: 'double-dispatch',
    chip: 'run the target twice',
    deed: 'dispatch the allowed function a second time in C-E2E-002.',
    log: [
      { text: 'generation 1 returns a function_call for <run_id>::record', tone: 'ok' },
      { text: 'target receives {value: "expected"} · fsynced, sequence 1', tone: 'ok' },
      { text: 'the same call is dispatched again · sequence 2', tone: 'bad' },
      { text: 'transcript: a single function_result, looks tidy', tone: 'dim' },
    ],
    oracles: {
      ...ALL_OK,
      target: { v: 'fail', note: '2 calls recorded, expected exactly 1' },
      transcript: { v: 'ok', note: 'looks clean, which is the point' },
    },
    classification: 'contract_failure',
    exit: '2',
    tone: 'alert',
    caughtBy: 'the target recorder',
    lesson: 'side effects do not show in a tidy transcript. the recorder fsyncs every accepted call before it responds, so the count cannot lie.',
  },
  {
    id: 'conflicting-terminals',
    chip: 'report two endings',
    deed: 'deliver turn-completed twice: once completed, once failed.',
    log: [
      { text: 'the turn completes and persists durably', tone: 'ok' },
      { text: 'lifecycle delivery 1: turn-completed · completed', tone: 'ok' },
      { text: 'lifecycle delivery 2: same turn · failed', tone: 'bad' },
      { text: 'durable status still says completed', tone: 'dim' },
    ],
    oracles: {
      ...ALL_OK,
      lifecycle: { v: 'fail', note: 'conflicting terminal payloads' },
      status: { v: 'ok', note: 'one durable truth' },
    },
    classification: 'contract_failure',
    exit: '2',
    tone: 'alert',
    caughtBy: 'the lifecycle recorder',
    lesson: 'delivery is at-least-once and unordered: identical duplicates are accepted, conflicting terminals never are.',
  },
  {
    id: 'stall',
    chip: 'stall forever',
    deed: 'accept the send, then never reach a terminal state.',
    log: [
      { text: 'probe ✓ · harness::send accepted', tone: 'ok' },
      { text: 'generation 1 streams · no terminal status follows', tone: 'warn' },
      { text: 'await: the completion deadline expires', tone: 'bad' },
      { text: 'status at the deadline: still running', tone: 'bad' },
    ],
    oracles: {
      ...ALL_OK,
      status: { v: 'fail', note: 'non-terminal at the deadline' },
      lifecycle: { v: 'skip', note: 'no terminal event arrived' },
    },
    classification: 'timeout',
    exit: '2',
    tone: 'alert',
    caughtBy: 'the await deadline',
    lesson: 'the only deadline that blames the subject is the one in await. every infrastructure deadline exits 3 instead.',
  },
  {
    id: 'crash',
    chip: 'kill the engine',
    deed: 'make the engine process die mid-turn.',
    log: [
      { text: 'send accepted · the turn starts', tone: 'ok' },
      { text: 'engine process exits unexpectedly, code 137', tone: 'bad' },
      { text: 'supervisor classifies the exit before any timeout', tone: 'warn' },
      { text: 'process-exit.json + stderr retained for 14 days', tone: 'dim' },
    ],
    oracles: {
      ...NOT_REACHED,
      send: { v: 'ok' },
      supervisor: { v: 'fail', note: 'unexpected exit mid-turn' },
    },
    classification: 'process_crash',
    exit: '3',
    tone: 'warn',
    caughtBy: 'the process supervisor',
    lesson: 'early exit is classified first, so a crash can never masquerade as a subject timeout.',
  },
  {
    id: 'unplug',
    chip: 'unplug context-manager',
    deed: 'start the stack without its required context worker.',
    log: [
      { text: 'boot base stack: context-manager is absent', tone: 'bad' },
      { text: 'probe: context::assemble has no live descriptor', tone: 'bad' },
      { text: 'readiness-failure.json names every missing surface', tone: 'warn' },
      { text: 'the run never sends', tone: 'dim' },
    ],
    oracles: NOT_REACHED,
    classification: 'setup_error',
    exit: '3',
    tone: 'warn',
    caughtBy: 'the readiness probe, before send',
    lesson: 'missing infrastructure stops the run, gets named, and exits 3. it is never a skip that reads as green.',
  },
  {
    id: 'bad-fixture',
    chip: 'ship a broken fixture',
    deed: 'author a script whose generation has no terminal frame.',
    log: [
      { text: 'compile scenario.yaml → generation 1 never ends', tone: 'bad' },
      { text: 'script validation rejects the fixture', tone: 'warn' },
      { text: 'nothing boots, nothing sends', tone: 'dim' },
    ],
    oracles: NOT_REACHED,
    classification: 'runner_error',
    exit: '3',
    tone: 'warn',
    caughtBy: 'script validation, before any process starts',
    lesson: 'a broken fixture is the author’s bug. it exits 3 and can never be pinned on the subject.',
  },
]
