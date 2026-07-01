export type WorkerKind = 'core' | 'sibling' | 'consumer' | 'substrate'

export interface FnDoc {
  id: string
  desc: string
}

export interface WorkerInfo {
  id: string
  kind: WorkerKind
  kindLabel: string
  role: string
  standalone: string
  install?: string
  functions: FnDoc[]
  emits?: FnDoc[]
  notes?: string[]
}

/**
 * worker datasheets, condensed from tech-specs/2026-06-agentic/*.md —
 * the source of truth for the system map side panel.
 */
export const WORKERS: Record<string, WorkerInfo> = {
  harness: {
    id: 'harness',
    kind: 'core',
    kindLabel: 'core worker',
    role: 'a thin, durable turn loop that wires the other three together — sequencing, and nothing else.',
    standalone:
      'the "assemble the agent" worker. any concern that grows real logic becomes a sibling worker, never harness code.',
    install: 'iii worker add harness',
    functions: [
      { id: 'harness::send', desc: 'accept a message, ensure the session, enqueue the first turn step. returns fast.' },
      { id: 'harness::run', desc: 'send, held open until the turn ends — call an agent like a function.' },
      { id: 'harness::spawn', desc: 'start a sub-agent in a child session; the parent call resolves with its result.' },
      { id: 'harness::stop', desc: 'cancel an in-flight turn; cascades to spawned children.' },
      { id: 'harness::status', desc: 'point-in-time read of a session\u2019s turn.' },
      {
        id: 'harness::function::trigger',
        desc: 'internal — run one model-requested call through policy, hooks, target.',
      },
      { id: 'harness::function::resolve', desc: 'internal — settle a parked call and resume the turn.' },
    ],
    emits: [
      { id: 'harness::turn_started', desc: 'a turn began executing.' },
      { id: 'harness::turn_completed', desc: 'terminal status + typed result. the orchestration surface.' },
      {
        id: 'harness::hook::*',
        desc: 'five synchronous in-path points: pre_turn, pre_generate, post_generate, pre_trigger, post_trigger.',
      },
    ],
    notes: [
      'depends softly on the other three: without context-manager it sends raw history; with no allow-list it is a plain chat loop.',
      'every loop step is a durable queue entry — a crash or restart resumes mid-turn.',
    ],
  },
  'session-manager': {
    id: 'session-manager',
    kind: 'core',
    kindLabel: 'core worker',
    role: 'the durable, reactive store of typed conversation entries — every mutation emits an event.',
    standalone: 'a real-time conversation store any app can subscribe to, with or without the loop around it.',
    install: 'iii worker add session-manager',
    functions: [
      { id: 'session::append', desc: 'append one entry; fires session::message-added.' },
      { id: 'session::update-message', desc: 'stream deltas into an entry; fires session::message-updated.' },
      { id: 'session::messages', desc: 'load the active path, oldest first.' },
      { id: 'session::set-status', desc: 'idle / working / done / error; fires session::status-changed.' },
      { id: 'session::fork', desc: 'branch history into a new session.' },
      { id: 'session::create / get / list / delete', desc: 'lifecycle, pagination, tenancy filters.' },
    ],
    emits: [
      { id: 'session::created', desc: 'a session exists.' },
      { id: 'session::message-added', desc: 'a new entry landed.' },
      { id: 'session::message-updated', desc: 'an entry\u2019s content changed (streaming).' },
      { id: 'session::status-changed', desc: 'working / done / error — drives spinners.' },
      { id: 'session::meta-updated', desc: 'title / metadata changed.' },
      { id: 'session::deleted', desc: 'a session was removed.' },
    ],
    notes: [
      'pure storage + notification surface: it binds nothing of its own and runs no agent logic.',
      'consumers reconcile by revision, never by arrival order.',
    ],
  },
  'context-manager': {
    id: 'context-manager',
    kind: 'core',
    kindLabel: 'core worker',
    role: 'turns raw history plus a target model into a model-ready context that fits the window.',
    standalone:
      'context budgeting for any ai feature — it owns no storage; callers pass message arrays in and persist the results.',
    install: 'iii worker add context-manager',
    functions: [
      { id: 'context::assemble', desc: 'system prompt + budgeted messages: count, prune, compact, fit.' },
      { id: 'context::compact', desc: 'summarise older history into one compaction summary.' },
      { id: 'context::prune', desc: 'strip verbose function output — the cheap first pass.' },
      { id: 'context::count-tokens', desc: 'estimate usage before committing to a model.' },
    ],
    notes: [
      'one summarisation per overflow, amortised — the harness persists the summary, so it never re-pays per turn.',
      'compaction leases make concurrent summarisation of one session mutually exclusive.',
    ],
  },
  'llm-router': {
    id: 'llm-router',
    kind: 'core',
    kindLabel: 'core worker',
    role: 'one front door in front of every llm provider — and the protocol provider workers implement.',
    standalone: 'provider-agnostic completions through one stable surface; swap providers with zero call-site changes.',
    install: 'iii worker add llm-router',
    functions: [
      { id: 'router::chat', desc: 'stream one assistant turn into a caller-supplied channel.' },
      { id: 'router::complete', desc: 'non-streaming one-shot; returns the final message.' },
      { id: 'router::abort', desc: 'cancel an in-flight stream by request id.' },
      {
        id: 'router::models::list / get / supports',
        desc: 'the live capability catalog, populated by provider discovery.',
      },
      {
        id: 'router::provider::register / resolve',
        desc: 'providers self-declare; credentials resolve centrally per request.',
      },
    ],
    emits: [],
    notes: [
      'consumer-agnostic: it assumes no harness, session, or ui.',
      'secrets never transit agent-visible surfaces — providers resolve credentials through the router, never from disk.',
      'typed errors, bounded retries, and cancellation are defined once, here.',
    ],
  },
  'approval-gate': {
    id: 'approval-gate',
    kind: 'sibling',
    kindLabel: 'optional sibling',
    role: 'the policy + decision surface for human-held function calls. plugs into the loop; never changes it.',
    standalone:
      'optional by design — the four core workers run with or without it. installing the worker is installing the policy.',
    install: 'iii worker add approval-gate',
    functions: [
      { id: 'approval::gate', desc: 'the pre_trigger hook: answer continue, deny, or hold.' },
      { id: 'approval::resolve', desc: 'apply a human decision — release the call or answer it with a denial.' },
      { id: 'approval::list_pending / get_pending', desc: 'the pending inbox, with tenancy filters.' },
      { id: 'approval::set_mode / approve_always', desc: 'per-session permission modes and standing grants.' },
    ],
    emits: [
      { id: 'approval::pending_created', desc: 'a call is waiting on a human — notify anywhere.' },
      { id: 'approval::pending_resolved', desc: 'the decision landed.' },
    ],
    notes: ['the inbox is ephemeral: records exist only while a call is held. the transcript is the audit trail.'],
  },
  chat: {
    id: 'chat',
    kind: 'consumer',
    kindLabel: 'example consumer',
    role: 'the console web chat — streams transcripts live and gates risky calls inline.',
    standalone:
      'every consumer is the same triangle: send through the harness, render from session events, observe turn boundaries.',
    functions: [
      { id: 'harness::send', desc: 'sends user messages (with session metadata for tenancy).' },
      { id: 'session::message-added / updated', desc: 'binds these to render deltas last-write-wins.' },
      { id: 'approval::resolve', desc: 'approve / deny buttons on held calls.' },
    ],
    notes: ['any worker or client can take this place — the surface is the contract.'],
  },
  'telegram-bot': {
    id: 'telegram-bot',
    kind: 'consumer',
    kindLabel: 'example consumer',
    role: 'a webhook bridge: telegram updates in, live message edits out.',
    standalone: 'same triangle as the chat, different surface — webhooks dedupe with an idempotency key.',
    functions: [
      { id: 'telegram::webhook', desc: 'receives updates; routes commands and messages.' },
      { id: 'harness::send', desc: 'idempotency_key = update_id — redeliveries change nothing.' },
      { id: 'session::message-updated', desc: 'bound to edit the telegram message (~1/s throttle).' },
    ],
  },
  'third-party': {
    id: 'third-party',
    kind: 'consumer',
    kindLabel: 'any worker',
    role: 'any worker on the bus — it can run the full loop, or call llm-router directly with no loop at all.',
    standalone: 'each core worker is independently useful; a consumer composes exactly the subset it needs.',
    functions: [
      { id: 'harness::run', desc: 'call an agent like a function — typed json result back.' },
      { id: 'router::chat', desc: 'or skip the loop and stream a completion directly.' },
    ],
  },
  substrate: {
    id: 'substrate',
    kind: 'substrate',
    kindLabel: 'the substrate',
    role: 'every registered function on the bus — the agent\u2019s reachable world.',
    standalone:
      'there is no separate capability layer to build or maintain: anything a worker registers, an agent can be allowed to reach.',
    functions: [
      { id: 'engine::functions::list', desc: 'how the model discovers what exists, at runtime.' },
      { id: 'shell::* / email::* / database::* / storage::* / …', desc: 'capability workers from the registry.' },
      { id: 'harness::spawn', desc: 'sub-agents are just one more function on the same surface.' },
    ],
    notes: ['fail-closed: with no allow-list, every call is refused. deployments opt functions in deliberately.'],
  },
}

/** hero stat strip */
export const STATS = [
  { value: '4+1', label: 'standalone workers' },
  { value: '15', label: 'trigger types emitted' },
  { value: '40+', label: 'functions on the surface' },
  { value: '1', label: 'invocation surface for the model' },
] as const
