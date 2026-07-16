/* flows — the conformance run (A5) and the agent-quality loop (A5). */
import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'

/* ---- conformance: one scenario, allocate to teardown ---- */

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
    label: 'start pinned stack',
    title: 'boot the real thing',
    desc: 'the pinned engine and real dependencies start in declared order: queue, state, session-manager, context-manager (required, fails closed), and the harness under test.',
  },
  {
    from: 'runner',
    to: 'engine',
    label: 'probe schemas',
    title: 'readiness, never sleep',
    desc: 'exact functions, request/response schemas, the harness::turn-completed trigger type, and the harness-turn queue topic must all be present. failure names every missing surface.',
  },
  {
    from: 'runner',
    to: 'router',
    label: 'load strict script',
    title: 'arm the model boundary',
    desc: 'the scripted router loads its generations. duplicate ordinals, invalid matchers, or a missing terminal frame reject the script before the stack is touched.',
  },
  {
    from: 'runner',
    to: 'recorder',
    label: 'reset + bind',
    title: 'arm the evidence',
    desc: 'the recorder registers the run-scoped target function, verifies its schema digest, clears its durable log, and binds the lifecycle trigger.',
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
    desc: 'the durable recorder log, ordered by strictly increasing sequence: target calls, arguments, counts, and lifecycle deliveries.',
  },
  {
    from: 'runner',
    to: 'runner',
    label: 'grade · report · teardown',
    title: 'pure assertions, then cleanup',
    desc: 'code invariants over the collected evidence, canonical json report, then sigterm, five seconds, sigkill. retention follows the classification.',
  },
]

/* ---- agent quality: one attempt, start to report ---- */

export const QUAL_LANES: SeqLane[] = [
  { id: 'ci', label: 'cli / ci', x: 100 },
  { id: 'eval', label: 'harness-eval', x: 300 },
  { id: 'harness', label: 'harness', x: 520 },
  { id: 'session', label: 'session-mgr', x: 730 },
  { id: 'validator', label: 'validator', x: 930 },
]

export const QUAL_STEPS: SeqStep[] = [
  {
    from: 'eval',
    to: 'harness',
    label: 'bind turn-completed',
    title: 'before any run exists',
    desc: 'the worker registers one unfiltered completion handler at startup and ignores events that match no running attempt. this kills the race between a fast turn and per-run binding.',
  },
  {
    from: 'ci',
    to: 'eval',
    label: 'harness-eval::start',
    title: 'a frozen manifest',
    desc: 'strict yaml, resolved and digested before start. the worker receives frozen content, never mutable local paths.',
  },
  {
    from: 'eval',
    to: 'eval',
    label: 'persist run + attempt',
    title: 'durable before send',
    desc: 'the run record, attempt, fixture setup, and cycle are persisted first, so a restart at any point recovers deterministically.',
  },
  {
    from: 'eval',
    to: 'harness',
    label: 'harness::send',
    title: 'the subject turn',
    desc: 'the scenario lowers to an ordinary public send with a deterministic idempotency key: sha-256 of run, leg, attempt, and cycle.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'persist',
    title: 'production path, end to end',
    desc: 'the pinned real model runs through the production router and provider; transcript and terminal state persist durably.',
  },
  {
    from: 'harness',
    to: 'eval',
    label: 'turn-completed',
    title: 'at-least-once, unordered',
    desc: 'duplicate deliveries are checkpoint no-ops. a periodic reconciler polls every non-terminal attempt, so a missed callback cannot strand a run.',
    event: 'harness::turn-completed',
  },
  {
    from: 'eval',
    to: 'harness',
    label: 'harness::status',
    title: 'reconcile to durable truth',
    desc: 'status and transcript are the recovery authority; the notification alone decides nothing.',
  },
  {
    from: 'eval',
    to: 'session',
    label: 'session::messages',
    title: 'collect all pages',
    desc: 'the full transcript becomes an artifact, exchanged with validators by reference and digest; the inline response cap is 1 MiB.',
  },
  {
    from: 'eval',
    to: 'validator',
    label: 'ValidationRequestV1',
    title: 'independent judgment',
    desc: 'each validator gets the fixture capabilities and token-scoped artifact access. validators are read-only by default and never see evaluator credentials.',
  },
  {
    from: 'validator',
    to: 'eval',
    label: 'ValidationResultV1',
    title: 'pass · fail · error · inconclusive',
    desc: 'fail means the subject missed the objective. error means the validator broke. missing evidence is never converted into a subject pass.',
  },
  {
    from: 'eval',
    to: 'harness',
    label: 'feedback cycle',
    title: 'bounded, same session',
    desc: 'only an open validator with continue_with_feedback and cycle budget left may trigger a new send in the same session, with the same pinned model, prompt, and options, and the next deterministic key.',
  },
  {
    from: 'eval',
    to: 'eval',
    label: 'publish report',
    title: 'raw dimensions, no composite',
    desc: 'validator results, metrics per leg, digests of manifest, prompt, and function catalog. required failures cannot be offset by lower latency or cost.',
  },
]
