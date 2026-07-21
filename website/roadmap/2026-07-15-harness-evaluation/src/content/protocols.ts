/* protocols — deep-dive data: recorder + supervisor (integration tests),
   helpers + evidence contracts (e2e tests). */

export const AUTHORING_LAYERS = [
  { name: 'AuthoredScenarioV1', type: 'what humans write', desc: 'one rust builder module per scenario: the send message, function aliases, typed replies (text / function_call / raw), deterministic defaults, an optional fault. no yaml layer — the type system validates authoring at cargo build.' },
  { name: 'CompiledFixtureV1', type: 'what the runner executes', desc: 'resolved before any process starts: all twelve router matchers explicit, literal wire frames, the derived recorder config, deadlines, and invariants. strict and self-contained.' },
  { name: 'compiled snapshots', type: 'reviewable in ci', desc: 'the canonical rendering of every authored scenario is checked into a snapshot test, so a compiler-default change reviews like an authored fixture change.' },
  { name: 'match_overrides · raw frames', type: 'escape hatches', desc: 'not the normal authoring path. raw frames receive the same terminal-frame and response/frame-consistency validation as compiler-generated ones.' },
] as const

export const RECORDER_PLANE = [
  { name: 'configure · reset · snapshot · await', type: 'in-process', desc: 'ordinary rust calls on a private service owned by the runner, never registered as iii functions. test setup stays out of the subject engine, and durable evidence stays readable after an engine crash.' },
  { name: '<run_id>::record', type: 'controlled target', desc: 'the run-scoped target functions are the first of two handler kinds that traverse the engine. the compiler derives each response schema as the exact-value draft 7 const of the declared response.' },
  { name: 'integration-recorder::lifecycle', type: 'lifecycle sink', desc: 'the second engine-visible handler, bound to harness::turn-completed. v1 records only the terminal event; the grader compares the delivered status with expect.terminal.status.' },
  { name: 'durable log', type: 'fsync before ack', desc: 'every target or lifecycle event is appended and fsynced before acknowledgement, with a strictly increasing sequence. await polls the same store; it is never a second evidence source.' },
  { name: 'no self-attestation', type: 'verified via engine', desc: 'the recorder does not attest its own registration. the runner independently queries engine::functions::info and requires the exact description and canonical schemas before send.' },
] as const

export const SUPERVISOR_STEPS = [
  'create a unique engine working/config directory with a filesystem configuration adapter',
  'reserve loopback ports; retry a bind race with a complete new port set',
  'apply an environment allowlist; provider keys and developer secrets are not inherited',
  'write per-worker seed yaml: unique session data_dir, context lease_dir, queue path, artifact dir',
  'start workers in declared order, stdout/stderr captured separately',
  'observe bounded immediate exits during boot; enforce readiness, scenario, collection, and teardown deadlines',
  'classify early process exit before any ordinary timeout',
  'sigterm, wait five seconds, sigkill the remaining process groups; write the typed teardown report',
] as const

export const QUALITY_HELPERS = [
  { fn: 'awaitTerminal(client, session_id, turn_id)', does: 'lifecycle events as the low-latency signal, harness::status as the authority; accepts duplicate and out-of-order deliveries, ignores terminal events for other turns in the session, and confirms the requested turn before returning' },
  { fn: 'sessionMetrics(client, session_id)', does: 'harness::session-tree + harness::metrics: usage summed over the root and every descendant, per session and in total. throws a typed error on complete: false — never a partial sum' },
  { fn: 'triggeredWork(client, session_id)', does: 'polls harness::triggered-work until complete or its deadline; throws when spans are dropped or open, parentage is malformed, or the subject-turn roots are wrong. assertions run only after that completeness check' },
  { fn: 'createSessionRegistry(client)', does: 'test-local bookkeeping with idempotent track() and stopNonTerminal(): cleanup reads durable status, stops only non-terminal sessions, confirms the terminal result, and throws if any tracked session cannot be stopped before the deadline' },
  { fn: 'eval-fixture::<profile>::setup / teardown', does: 'ordinary iii functions with idempotent run-scoped keys, called from beforeAll/afterAll; teardown is safe to repeat and failure retains the namespace for inspection' },
  { fn: 'runId()', does: 'the launcher-supplied stack identity every idempotency, fixture, and state key derives from, so a retried run cannot double-apply side effects' },
] as const

export const EVIDENCE_CONTRACTS = [
  {
    name: 'harness::session-tree',
    type: 'versioned public read · V1',
    desc: 'the recovery authority for tree membership: the root at depth zero plus every dispatcher-linked or reactive descendant, each parent relation persisted before the child becomes runnable. complete: false means the set is not provably exhaustive.',
  },
  {
    name: 'harness::metrics',
    type: 'versioned public read · V1',
    desc: 'turns, function calls, errors, tokens, and cost summed over that tree, with a per-session breakdown, root first. when any descendant transcript is unavailable it sets complete: false and helpers refuse to grade it.',
  },
  {
    name: 'harness::triggered-work',
    type: 'versioned public read · V1',
    desc: 'the complete retained span set rooted at the subject turns: function, hook, trigger, and sub_agent spans with worker, session, status, and parentage. complete: true only with zero drops, zero open spans, and no query truncation. payloads and arbitrary attributes are omitted, so evidence never grows a second secret-bearing surface.',
  },
  {
    name: 'trace propagation',
    type: 'default behavior',
    desc: 'the harness propagates the subject turn’s trace context to every function call, sub-agent turn, hook, and triggered handler. not an evaluation feature: the same context production observability reads.',
  },
] as const

export const AUTHORING_RULES = [
  {
    name: 'only public api',
    desc: 'a test uses worker.trigger on public iii and harness functions plus the helper package. a helper that only renames a single existing call is outside the package contract.',
  },
  {
    name: 'explicit subject, reused verbatim',
    desc: 'harness defaults re-resolve on every send, so the subject object pins model, provider, prompt strategy, and every option once; every send spreads that same object. a test relying on a default sends exactly once.',
  },
  {
    name: 'sequences and feedback are code',
    desc: 'a prompt sequence is the next send after the prior terminal turn; a feedback loop is a bounded loop with its maximum iteration count declared in the test file.',
  },
  {
    name: 'deterministic identity',
    desc: 'every idempotency, fixture, and state key derives from the launcher-supplied run id plus a test-local suffix, so a retried test run cannot double-apply side effects.',
  },
  {
    name: 'cleanup is mandatory',
    desc: 'every send response is tracked immediately in the session registry; afterEach stops any non-terminal session with harness::stop, and a vitest timeout bounds every case. token and cost ceilings are post-turn assertions, never hard preemption.',
  },
  {
    name: 'custom checks are functions',
    desc: 'a reusable domain check is a registered iii function the test calls with worker.trigger and asserts on — not a validator protocol with its own lifecycle.',
  },
] as const

export const V1_CORPUS = [
  { family: 'plain response', outcome: 'durable final text with no duplicate assistant entry' },
  { family: 'single function', outcome: 'the allowed target executes once and its result reaches the next generation' },
  { family: 'sub-agent fan-out/fan-in', outcome: 'children complete, the parent waits for all required results, and every child’s usage appears in by_session' },
  { family: 'triggered work', outcome: 'declared reactive orchestration is visible in trace spans and error-free' },
] as const

export const POST_V1_CORPUS = [
  { family: 'parallel functions', outcome: 'independent calls finish without loss or duplication' },
  { family: 'multi-prompt conversation', outcome: 'each scripted send follows the prior terminal turn; the final state reflects every input in order' },
  { family: 'persistent workflow', outcome: 'external records match processed fixture items exactly' },
  { family: 'browser workflow', outcome: 'url, dom, network, console, and screenshot evidence agree' },
  { family: 'recovery', outcome: 'a dependency failure is surfaced and bounded rather than hidden' },
] as const

export const EXCLUDED_CAPABILITIES = [
  {
    name: 'baseline/candidate comparison',
    desc: 'paired scheduling, per-dimension deltas, and eligibility rules. the raw assets already allow comparing two runs by hand; the machinery waits until repeated single-subject runs establish variance.',
  },
  {
    name: 'held-out + generated validators',
    desc: 'checks invisible to the subject, and checks generated from a frozen goal by a pinned model, need their own trust and isolation design before any release authority.',
  },
  {
    name: 'production/runtime evaluation',
    desc: 'evaluating a production session is pulling its metrics, traces, and transcript by session id and grading them — the same reads this suite uses. no dedicated feature belongs to version 1.',
  },
  {
    name: 'an orchestrator worker',
    desc: 'a durable harness-eval worker (long-running runs, comparison legs at scale, retry-safe run records) is outside version 1.',
  },
] as const

export const TRUST_RULES = [
  {
    name: 'no self-grading',
    desc: 'assertions read durable records and fixture state, never the agent’s self-report. outcome correctness stays independent of the subject’s claims.',
  },
  {
    name: 'no model judges',
    desc: 'an llm is only ever the subject, never the judge. every check is explicit code a reviewer can read in the test file.',
  },
  {
    name: 'scoped fixtures',
    desc: 'each fixture adapter proves its own tenant, database, filesystem, and state isolation before it can be shared between test files.',
  },
  {
    name: 'secret hygiene',
    desc: 'provider credentials enter only through an environment allowlist and never appear in config, artifacts, or persisted evidence; failure messages are redacted before persistence.',
  },
] as const
