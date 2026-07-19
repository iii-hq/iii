/* protocols — deep-dive data: recorder + supervisor (integration), worker
   surface + recovery (agent quality). */

export const RECORDER_FUNCTIONS = [
  { fn: 'integration-recorder::configure', does: 'registers the run-scoped target verbatim and returns its canonical schema digest; the registration itself is part of the oracle' },
  { fn: 'integration-recorder::reset', does: 'clears only the current run; idempotent; next_sequence returns to 1' },
  { fn: 'integration-recorder::snapshot', does: 'the durable event log, ordered by strictly increasing sequence' },
  { fn: 'integration-recorder::await', does: 'a deadline-bounded convenience over the same durable log, never a second evidence source' },
  { fn: 'integration-recorder::lifecycle', does: 'receives the exact harness lifecycle payload for turn-started / turn-completed bindings' },
] as const

export const SUPERVISOR_STEPS = [
  'create a unique engine working/config directory with a filesystem configuration adapter',
  'reserve loopback ports; retry a bind race with a complete new port set',
  'apply an environment allowlist; provider keys and developer secrets are not inherited',
  'write per-worker seed yaml: unique session data_dir, context lease_dir, queue path, artifact dir',
  'start workers in declared order, stdout/stderr captured separately',
  'enforce startup, readiness, scenario, collection, and teardown deadlines',
  'classify early process exit before any ordinary timeout',
  'sigterm, wait five seconds, sigkill the remaining children',
] as const

export const CASSETTE_FIELDS = [
  { name: 'captured_at · revisions', type: 'provenance', desc: 'engine, harness, and router revisions recorded at capture; capture is manual and non-gating.' },
  { name: 'script', type: 'RouterScriptV1', desc: 'a sanitized script: the same schema the scripted router replays.' },
  { name: 'sanitized_sha256', type: 'digest', desc: 'sha-256 over canonical json with the digest field omitted.' },
  { name: 'denylist scan', type: 'gate', desc: 'credentials, cookies, personal data, unstable trace/session/request ids, provider-private metadata. a cassette commits only after the scan and a schema round-trip pass.' },
] as const

export const EVAL_SURFACE = [
  { fn: 'harness-eval::start', does: 'accepts a single scenario or a baseline/candidate comparison (a tagged union: exactly one), returns a running run_id' },
  { fn: 'harness-eval::status', does: 'run state plus per-attempt leg, cycle, session, and result' },
  { fn: 'harness-eval::report', does: 'the full report: digests, subject snapshots, validator results, metrics, deltas; an error until terminal' },
  { fn: 'harness-eval::cancel', does: 'stops every running attempt via harness::stop, reconciles to terminal; idempotent' },
  { fn: 'harness-eval::artifact::put / get', does: 'token-scoped, content-addressed evidence exchange for the evaluator and validators only, denied to the subject' },
] as const

export const ERROR_CODES = [
  { code: 'harness-eval/invalid_manifest', meaning: 'schema, range, function policy, or dependency declaration is invalid' },
  { code: 'harness-eval/run_not_found', meaning: 'no durable run exists for the supplied id' },
  { code: 'harness-eval/run_not_terminal', meaning: 'a final report was requested before terminal state' },
  { code: 'harness-eval/budget_exceeded', meaning: 'a declared time, token, cost, attempt, or cycle budget ended the run' },
  { code: 'harness-eval/dependency', meaning: 'harness, transcript, validator, browser, trace, or artifact dependency failed' },
  { code: 'harness-eval/internal', meaning: 'evaluator state or invariant failure' },
] as const

export const RECOVERY_TABLE = [
  { state: 'setup request exists, result missing', action: 'repeat fixture setup with the same idempotency key' },
  { state: 'setup persisted, no cycle', action: 'validate capabilities, then create cycle 1' },
  { state: 'cycle exists, no send identity', action: 'repeat harness::send with the same idempotency key' },
  { state: 'session/turn known, non-terminal', action: 'poll harness::status until the deadline' },
  { state: 'terminal status, validation missing', action: 'fetch all transcript pages and run validators' },
  { state: 'validation persisted, continuation missing', action: 'apply the recorded decision once' },
  { state: 'attempt terminal, teardown missing', action: 'repeat teardown with the same idempotency key' },
  { state: 'final report persisted', action: 'return it; never rerun validators implicitly' },
] as const

export const SCENARIO_CORPUS = [
  { family: 'plain response', outcome: 'durable final text, no duplicate assistant entry' },
  { family: 'single function', outcome: 'the allowed target executes once; its result reaches the next generation' },
  { family: 'parallel functions', outcome: 'independent calls finish without loss or duplication' },
  { family: 'sub-agent fan-out/fan-in', outcome: 'children complete, the parent waits for all required results, and every child’s usage is attributed in the report' },
  { family: 'multi-prompt conversation', outcome: 'each scripted input sends only after the prior turn is terminal; the final state reflects every input in order' },
  { family: 'persistent workflow', outcome: 'external records match processed fixture items exactly' },
  { family: 'browser workflow', outcome: 'url, dom, network, console, and screenshot evidence agree' },
  { family: 'recovery', outcome: 'a dependency failure is surfaced and bounded rather than hidden' },
  { family: 'prompt comparison', outcome: 'identical frozen inputs; raw deltas reported' },
] as const

export const GENERATED_VALIDATOR_RULES = [
  {
    name: 'declared as a goal, not code',
    desc: 'the frozen manifest carries the generator input: goal text, a pinned generator model, and the evidence classes the code may read. never the code itself.',
  },
  {
    name: 'frozen before the subject turn',
    desc: 'generation runs once per run, before the first send. the source is persisted, digested, and recorded in the report before any subject work exists; it is never regenerated after the first send.',
  },
  {
    name: 'run-scoped registration',
    desc: 'a disposable, secret-free validator-host registers eval-gen::<run_id>::<slug> through normal iii registration; the namespace is denied to the subject and unregistered at teardown.',
  },
  {
    name: 'never the sole gate',
    desc: 'in v1 a generated required validator is valid only next to a provided required one. a generation, registration, or digest failure is error, never a subject pass.',
  },
  {
    name: 'one digest per comparison',
    desc: 'a comparison generates once and judges both legs with the same code digest; legs with differing digests are ineligible. generator usage bills to the evaluation, never the subject.',
  },
] as const

export const TRUST_RULES = [
  {
    name: 'held-out validators',
    desc: 'never contribute function metadata, schema, prompt text, or feedback to the subject. the subject cannot optimize for a grader it cannot see.',
  },
  {
    name: 'artifact tokens',
    desc: 'random, attempt-scoped, validator-scoped capabilities. never in the subject prompt, transcript, metadata, catalog, or artifacts; revoked after each validator call.',
  },
  {
    name: 'subject function policy',
    desc: 'harness-eval::*, eval-private::*, and eval-gen::* are explicitly denied, even when the allow pattern is broad.',
  },
  {
    name: 'hard vs soft ceilings',
    desc: 'wall-clock, cycle, and attempt limits are hard: harness::stop at the deadline. token, cost, browser, and network counts are post-turn soft ceilings that may overshoot by one bounded unit, recorded, never mislabeled as hard.',
  },
] as const
