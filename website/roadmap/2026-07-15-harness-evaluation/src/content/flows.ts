/* flows — the integration run (A5) and the agent-quality loop (A5). */
import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

/* ---- integration: one scenario, allocate to teardown ---- */

export const CONF_LANES: SeqLane[] = [
  { id: 'runner', label: 'runner', x: 80 },
  { id: 'engine', label: 'engine', x: 224 },
  { id: 'queue', label: 'queue', x: 368 },
  { id: 'harness', label: 'harness', x: 512 },
  { id: 'router', label: 'scripted router', x: 656 },
  { id: 'session', label: 'session-mgr', x: 800 },
  { id: 'recorder', label: 'recorder', x: 944 },
]

export const CONF_STEPS: SeqStep[] = [
  {
    from: 'runner',
    to: 'runner',
    label: 'allocate',
    title: 'a fresh world',
    desc: 'unique run id, working directory, isolated data dirs, reserved loopback ports, deadlines. nothing is reused from any previous scenario.',
  },
  {
    from: 'runner',
    to: 'engine',
    label: 'boot base stack',
    title: 'boot the real thing, minus the subject',
    desc: 'the pinned engine and real dependencies start in declared order: queue, state, session-manager, context-manager (required, fails closed), and the scripted router. the harness does not boot yet.',
  },
  {
    from: 'runner',
    to: 'engine',
    label: 'probe base contracts',
    title: 'readiness, never sleep',
    desc: 'live descriptors through engine::functions::info, compared canonically against checked-in goldens, plus the configuration seed, trigger types, and the harness-turn queue topic. failure names every mismatched surface.',
  },
  {
    from: 'runner',
    to: 'router',
    label: 'load strict script',
    title: 'arm the model boundary',
    desc: 'the compiled script loads its generations. duplicate ordinals, invalid matchers, or a missing terminal frame reject the fixture before the stack is touched.',
  },
  {
    from: 'runner',
    to: 'recorder',
    label: 'configure + reset',
    title: 'arm the evidence',
    desc: 'the recorder control plane is in-process: ordinary rust calls reset the durable log and configure the run-scoped target. nothing about test setup traverses the subject engine.',
  },
  {
    from: 'recorder',
    to: 'engine',
    label: 'register target',
    title: 'the one controlled function',
    desc: 'the run-scoped target (like <run_id>::record) registers through the engine with the authored description and schema, verbatim.',
  },
  {
    from: 'runner',
    to: 'engine',
    label: 'verify live descriptor',
    title: 'no self-attestation',
    desc: 'the runner independently queries engine::functions::info: exact description, canonically equal request schema, and the compiler-derived constant response schema must all match before anything sends.',
  },
  {
    from: 'runner',
    to: 'engine',
    label: 'boot + probe harness',
    title: 'the subject arrives last',
    desc: 'native discovery can snapshot registrations while a worker starts, so the harness boots only after the target is armed and verified. its live contracts get the same probe.',
  },
  {
    from: 'runner',
    to: 'engine',
    label: 'bind lifecycle sink',
    title: 'the last piece of arming',
    desc: 'the harness::turn-completed binding to integration-recorder::lifecycle is installed and verified only after both the base stack and the harness pass readiness.',
  },
  {
    from: 'runner',
    to: 'harness',
    label: 'harness::send',
    title: 'one ordinary public turn',
    desc: 'the exact request and response are recorded. no private state is seeded; harness::turn is never called.',
  },
  {
    from: 'harness',
    to: 'queue',
    label: 'enqueue harness-turn',
    title: 'through the real queue',
    desc: 'the turn rides the same fifo work delivery production uses.',
    event: 'harness-turn',
  },
  {
    from: 'queue',
    to: 'harness',
    label: 'durable step',
    title: 'the turn loop runs',
    desc: 'the durable turn loop picks up the work item and drives the turn.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'persist',
    title: 'durable before visible',
    desc: 'messages and state persist through session-manager; the transcript is the durable order/content authority.',
  },
  {
    from: 'harness',
    to: 'router',
    label: 'router::chat',
    title: 'the scripted generation',
    desc: 'the harness calls router::chat exactly as it would call the production router. every request field is matched against the script; there is no runner default.',
  },
  {
    from: 'router',
    to: 'harness',
    label: 'ordered frames',
    title: 'a frozen stream',
    desc: 'the script emits its exact AssistantMessageEvent frames (text deltas, usage, stop, done), then the terminal response, only after terminal streaming has been relayed.',
  },
  {
    from: 'harness',
    to: 'recorder',
    label: 'turn-completed',
    title: 'lifecycle, witnessed',
    desc: 'the completion notification is at-least-once and unordered. identical duplicates are accepted; conflicting terminals fail the scenario.',
    event: 'harness::turn-completed',
  },
  {
    from: 'runner',
    to: 'harness',
    label: 'harness::status',
    title: 'confirm, don’t trust',
    desc: 'the event is not the only source of truth: the runner confirms terminal durable status against harness::status.',
  },
  {
    from: 'runner',
    to: 'session',
    label: 'session::messages',
    title: 'the whole transcript',
    desc: 'every page, following next_cursor until absent. duplicate or missing entries are exactly what this evidence catches.',
  },
  {
    from: 'runner',
    to: 'recorder',
    label: 'snapshot',
    title: 'target + lifecycle evidence',
    desc: 'the durable recorder log, fsynced before every acknowledgement and ordered by strictly increasing sequence: target calls, arguments, counts, and lifecycle deliveries.',
  },
  {
    from: 'runner',
    to: 'runner',
    label: 'grade · teardown · report',
    title: 'pure assertions, typed cleanup',
    desc: 'code invariants over the collected evidence, then sigterm, five seconds, sigkill with a typed teardown report. the stable result.json and volatile execution.json are written last, linked by sha-256.',
  },
]

/* ---- agent quality: one test, send to verdict ---- */

export const QUAL_LANES: SeqLane[] = [
  { id: 'test', label: 'vitest test', x: 100 },
  { id: 'helpers', label: 'harness-test', x: 300 },
  { id: 'harness', label: 'harness', x: 520 },
  { id: 'session', label: 'session-mgr', x: 730 },
  { id: 'obs', label: 'observability', x: 930 },
]

export const QUAL_STEPS: SeqStep[] = [
  {
    from: 'test',
    to: 'test',
    label: 'fixture setup',
    title: 'idempotent, run-scoped',
    desc: 'beforeAll triggers the fixture setup function with a key derived from the launcher-supplied run id. a retried run cannot double-apply side effects.',
  },
  {
    from: 'test',
    to: 'harness',
    label: 'harness::send',
    title: 'the api as-is',
    desc: 'trigger("harness::send") with an explicit subject object: pinned model, provider, prompt strategy, and every option. no wrapper, no manifest — the same call production orchestration code would make.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'persist',
    title: 'production path, end to end',
    desc: 'the pinned real model runs through the production router and provider; transcript and terminal state persist durably.',
  },
  {
    from: 'test',
    to: 'helpers',
    label: 'awaitTerminal',
    title: 'events signal, status decides',
    desc: 'lifecycle events are the low-latency signal; the helper accepts duplicate and out-of-order deliveries and always confirms terminal state through harness::status before returning.',
    event: 'harness::turn-completed',
  },
  {
    from: 'test',
    to: 'harness',
    label: 'next send, same session',
    title: 'sequences and feedback are code',
    desc: 'a prompt sequence is the next send after the prior turn is terminal; a feedback loop is an ordinary bounded loop, its bound visible in the file. every send reuses the same subject object verbatim.',
  },
  {
    from: 'test',
    to: 'harness',
    label: 'trigger domain reads',
    title: 'durable outcomes, not claims',
    desc: 'the test reads fixture state through the same public functions any worker would call, then asserts with plain expect(). the agent’s self-report decides nothing.',
  },
  {
    from: 'test',
    to: 'helpers',
    label: 'sessionMetrics',
    title: 'the whole tree or nothing',
    desc: 'the helper aggregates usage over the root and every descendant session. complete: false throws a typed error — a partial sum is never graded.',
  },
  {
    from: 'helpers',
    to: 'harness',
    label: 'session-tree + metrics',
    title: 'default harness assets',
    desc: 'harness::session-tree and harness::metrics are proposed public reads: tree membership, then turns, calls, errors, tokens, and cost summed per session and in total. the same reads production orchestration needs.',
  },
  {
    from: 'helpers',
    to: 'obs',
    label: 'triggeredWork → spans',
    title: 'traces are first-class',
    desc: 'trace spans propagated from the subject turn count the work the session caused in other workers. a test asserting triggered work fails closed when spans are missing or dropped.',
  },
  {
    from: 'test',
    to: 'test',
    label: 'expect() · cleanup',
    title: 'explicit verdict, typed cleanup',
    desc: 'assertions over outcomes, metrics, and spans give the verdict. afterEach stops any non-terminal session with harness::stop, and helpers persist the evidence directory as they run.',
  },
]
