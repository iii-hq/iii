# harness

Worker prefix: `harness::*`

## Definition

`harness` is the thin worker that wires the other three into an agent loop. It owns sequencing and
nothing else: take an incoming message, persist it, assemble a context, stream a completion, persist
the result, execute any function calls, and repeat until the turn stops.

It is deliberately minimal. The rule of thumb: **if a concern grows real logic, it becomes its own
worker** rather than living in the harness. Approval gating, spend budgets, and compaction
*scheduling* are all out of scope here — they are siblings the harness can call or that can
subscribe around it (see [Out of scope](#out-of-scope-future-sibling-workers)). Three extension
surfaces do live here, because they need the loop's durability machinery: **deferred function
results** (a dispatch may park the turn and resolve later — see
[Deferred trigger](#deferred-trigger-pending-function-results)), **sub-agent spawning** built on
top of it (see [Sub-agents](#sub-agents-harnessspawn)), and **hooks** (the synchronous points where
siblings veto, hold, or mutate in-path — see [Hooks](#hooks)). Children are ordinary harness
sessions/turns; orchestration beyond spawn/join stays out of scope, and hook *logic* (the policy
itself) always lives in the sibling that registers it.

The harness is the only worker in this spec that depends on the other three, and even those are soft:
without `context-manager` it sends raw history; without function dispatch it is a plain chat loop. It
needs `session-manager` (to persist/stream) and `llm-router` (to generate).

## What it wires

```mermaid
sequenceDiagram
  participant C as consumer (chat/tg)
  participant H as harness
  participant S as session-manager
  participant X as context-manager
  participant R as llm-router
  participant F as iii function

  C->>H: harness::send {message, model}
  H->>S: session::create or ensure
  H->>S: session::append (user message)
  H-->>C: {session_id, turn_id}
  Note over H: enqueue harness::turn (durable)
  H->>S: session::set-status working
  H->>S: session::messages
  H->>X: context::assemble
  opt assemble compacted the head
    H->>S: session::append (custom compaction entry)
  end
  H->>R: router::chat (over channel)
  R-->>H: AssistantMessageEvent frames
  H->>S: session::append (assistant) then session::update-message (stream deltas)
  alt assistant requested function calls
    H->>F: iii.trigger(function_id, args)
    F-->>H: result
    H->>S: session::append (function_result)
    Note over H: re-enqueue harness::turn
  else pending trigger (e.g. harness::spawn)
    Note over H: park turn — child session runs its own loop;<br/>harness::function::resolve re-enqueues
  else no function calls
    H->>S: session::set-status done
  end
```

## The loop

`harness::send` is the entry point; it ensures the session (applying `session.metadata`, the
tenancy hook), persists the user message, and enqueues the first `harness::turn` step, then returns
immediately — or merges into a turn that is already running (see
[Concurrency & steering](#concurrency--steering)). [`harness::spawn`](#harnessspawn) seeds a child
session through the same CAS. The loop runs as durable enqueued
steps so a crash or restart resumes mid-turn (see
[Durability & idempotency](#durability--idempotency)). Every `session::append` /
`session::update-message` the loop issues carries `origin: { turn_id }`, so session events are
attributable to a turn. One `harness::turn` step does:

1. Mark working: `session::set-status working` and emit
   [`harness::turn_started`](#trigger-types-emitted) (first step of a turn), then run the
   `pre_turn` [hook chain](#hooks) — a `deny` ends the turn (`failed`, with the hook's reason)
   before any model spend.
2. Load active path: `session::messages` with `include_custom: true` (custom entries carry the
   compaction record, below).
3. Assemble context: read the latest compaction entry (if any) on the active path, reduce the
   candidate window to it, and call `context::assemble` with `previous_summary` set (see
   [Compaction persistence](#compaction-persistence)); skipped if `context-manager` absent -> raw
   messages + base system prompt. If the response reports `applied.compacted`, persist the new
   summary (same section). Attach the invocation schemas to the router request: the single
   `agent_trigger` schema by default (function discovery is runtime — the model calls
   `engine::functions::list` / `engine::functions::info` through it), or one schema per allowed
   function when `functions.expose: "native"` (see
   [Functions (the white box)](#functions-the-white-box)) — plus the synthetic `submit_result`
   schema when an [output contract](#output-contract) uses the fallback strategy. Finally run the
   `pre_generate` [hook chain](#hooks) over the assembled context — hooks may extend the system
   prompt or append bounded messages (memory, RAG, guardrails); a `deny` ends the turn as in
   step 1.
4. Generate: open a channel, call `router::chat` with `request_id = <turn_id>:<step>` (recorded on
   the turn record as `stream_request_id` for [`harness::stop`](#harnessstop)) and — when the
   [output contract](#output-contract) rides provider-native structured output —
   `response_format`; `session::append` an
   assistant message, then `session::update-message` as deltas arrive (each fires
   `session::message-updated`). Deltas may be batched to throttle update frequency; the final update
   writes the complete `AssistantMessage`. After the final update, run the read-only
   `post_generate` [hook chain](#hooks) (usage accounting, safety logging).
5. If the message has `function_call` content: a `submit_result` call (present only when an
   [output contract](#output-contract) is active) is consumed by the harness itself — validate,
   record the result on the turn record, and finalise as in step 6. Everything else is an
   `agent_trigger` call: unwrap each, trigger via `harness::function::trigger` sequentially in
   content order (the schema declares `execution_mode: "sequential"`) — each trigger runs the
   glob policy, then the `pre_trigger` [hook chain](#hooks), then the target, then the
   `post_trigger` chain over the result — and append each
   `function_result` — checkpointing per call (see
   [Durability & idempotency](#durability--idempotency)). A trigger may report **pending** instead
   of returning a result (a `pre_trigger` *hold*, or e.g.
   [`harness::spawn`](#harnessspawn)): the call checkpoints as
   `pending` and, once the trigger pass ends, the turn **parks** — the step ends without
   re-enqueueing, and [`harness::function::resolve`](#harnessfunctionresolve) resumes the loop
   later (see [Deferred trigger](#deferred-trigger-pending-function-results)). With no pending
   calls, re-enqueue `harness::turn` to let the model react.
6. Else, steering check: re-read `session::messages` for user-role entries after the turn record's
   `watermark_entry_id` (see [Concurrency & steering](#concurrency--steering)); if present, continue
   with another generate step. Otherwise finalise: resolve the turn `result` per the
   [output contract](#output-contract) (a schema-bearing contract with no valid result yet nudges
   instead, bounded), mark the turn `completed`, `session::set-status done`, emit
   [`harness::turn_completed`](#trigger-types-emitted), and — for a sub-agent turn — resolve the
   parent's pending call (see [Sub-agents](#sub-agents-harnessspawn)).

A `max_turns` guard caps runaway loops (turn ends `completed` with a synthetic notice). Cancellation
is cooperative *between* steps and explicit *during* generation: `harness::stop` sets an abort flag
the next step checks, and when a stream is in flight it also calls
[`router::abort`](llm-router.md#routerabort) with the `stream_request_id` recorded on the turn
record. The generate step then finalises the partial assistant message (`stop_reason: "aborted"`),
records `TurnStatus` `cancelled`, and sets `session::set-status done`. When the turn has live
spawned children, the stop cascades to them before the turn finalises (see
[Sub-agents](#sub-agents-harnessspawn)).

The harness maps the turn lifecycle onto the session's coarse status: `working` while a turn is
running or awaiting functions, `done` when it ends `completed` or `cancelled`, and `error` (with a
short `reason`) when it ends `failed`. The internal `TurnStatus` (below) is finer-grained and stays
inside the harness; consumers watch the session status, bind
[`harness::turn_completed`](#trigger-types-emitted) for turn outcomes (terminal status + result),
or call `harness::status` when they need a point-in-time read.

## Compaction persistence

`context-manager` is stateless — if nobody persists its compaction output, every turn past the
budget re-summarises the whole head (one extra LLM call per turn) and summaries never converge. The
harness is the caller, so the harness persists:

- When `context::assemble` returns `applied.compacted: true`, the harness appends a `custom` session
  entry — `{ custom_type: "compaction", data: { summary, tail_start_entry_id, tokens_before } }` —
  mapping `applied.tail_start_index` onto the entry id of the loaded active path.
- At the start of every assemble (loop step 3), the harness scans the loaded path for the **latest**
  compaction entry. When present, the candidate window passed to `context::assemble` is only the
  messages from `tail_start_entry_id` onward (compaction entries themselves are never sent), with
  `options.previous_summary` set to the stored summary so a re-compaction updates it in place.
- `options.lease_key` is always the `session_id`, so concurrent compactions of one session are
  mutually excluded across workers.

Result: one summarisation per overflow, amortised — not one per turn. The durable transcript is
untouched; the compaction entry is loop bookkeeping the harness owns (see
[context-manager.md § The compaction round trip](context-manager.md#the-compaction-round-trip)).

## Durability & idempotency

The `harness-turn` queue is **at-least-once**: any step may be redelivered after a crash, and every
step must tolerate it. The rules:

- **Stale-step guard.** Each dequeue compares `payload.step` to the turn record's current `step`; a
  lower step is acked and dropped. The guard only catches *old* steps — redelivery of the *current*
  step while it is still executing is indistinguishable from a resume, so the queue's
  visibility/processing timeout MUST exceed the worst-case step duration (which the router's stream
  idle timeout bounds — see
  [llm-router.md § Stream liveness](llm-router.md#stream-liveness-and-cancellation)). [Hook](#hooks)
  chains run inside the step, so their `timeout_ms` budgets count toward this bound too; a
  `pre_trigger` *hold* does not — it parks the turn instead of blocking the step.
- **Deterministic entry ids.** Every entry a step writes uses a deterministic id supplied via
  `session::append`'s `entry_id` (idempotent: appending an existing id is a no-op). The assistant
  message of a generate step is `e_<turn_id>_<step>_assistant`; a `function_result` is
  `e_<turn_id>_<function_call_id>`. A redelivered step therefore writes into the same entries
  instead of duplicating them: if the deterministic assistant entry already exists, the resumed
  generate step streams into it via `session::update-message` rather than appending a second
  message — a crash never yields two assistant messages.
- **Per-call checkpoints.** The turn record carries
  `calls: Record<function_call_id, { state: "triggered" | "pending" | "done"; entry_id?: string;
  child_session_id?: string; child_turn_id?: string; held_by?: string }>`. The trigger loop
  checkpoints `triggered`
  *before* invoking the target function, `pending` when the trigger defers its result (see
  [Deferred trigger](#deferred-trigger-pending-function-results)), and `done` *after* the
  `function_result` entry is appended. On redelivery: `done` calls are skipped; `pending` calls stay
  parked (their result arrives via `harness::function::resolve`, idempotent on the deterministic
  entry id); a call found `triggered` but not `done` is **not re-invoked** — the side effect may or
  may not have happened, so the harness appends a synthetic `function_result` with `is_error: true`
  (`"interrupted: executed at most once, result unknown (restart during execution)"`) and lets the
  model decide whether to retry. Step delivery is at-least-once; function side effects are
  at-most-once.
- **Status writes** (`session::set-status`, turn record transitions) are naturally idempotent —
  re-setting the same value is a no-op and fires no event.

## Concurrency & steering

One turn per session, enforced at the entry point:

- **Turn CAS.** `harness::send` seeds the turn record with an atomic check-and-set: it creates a new
  turn only if no record exists or the existing record is terminal (`completed` / `cancelled` /
  `failed`). Two concurrent sends create exactly one turn — the loser of the CAS takes the merge
  path.
- **Merge path.** If a turn is already `running` / `awaiting_functions`, `harness::send` only
  appends the user message and returns the running turn's id with `merged: true`. The running loop's
  steering check folds the message in. A merged send never changes the running turn's `model`,
  `system_prompt`, or `functions` policy — per-send options are stored on the turn record when the
  turn is created and apply unchanged until it ends.
  **Merge double-check.** The append races the loop's completion: the steering check (step 6) may
  read before the append and complete after it, which would strand the message until the next send.
  So after appending, the merge path re-reads the turn record — if the turn went terminal in that
  window, it re-runs the CAS and starts a fresh turn for the appended message. A merged send is
  never silently dropped.
- **Steering watermark.** The turn record stores `watermark_entry_id` — the active-path leaf
  observed when the latest generate step assembled its context. The steering check (loop step 6)
  asks `session::messages` for user-role entries **after the watermark**; if any exist it continues
  with another generate step (advancing the watermark), otherwise the turn completes. "Arrived after
  this turn started" is defined by entry position, never wall-clock time.

## Functions (the white box)

Functions are not a harness feature — they are the **iii substrate**. Any registered iii function is
callable; the harness **does not** map registry entries into provider tool schemas.

The harness:

- Attaches **one** invocation schema (`agent_trigger`) to each `router::chat` request by default,
  so the model can trigger any allowed function via `{ function, payload }` (per-function schemas
  in native exposure mode — see [Exposure modes](#functions-the-white-box) below).
- On `function_call` content: unwraps `agent_trigger` → target `function_id` + `payload`, enforces
  the dispatch policy below, triggers via `iii.trigger({ function_id, payload })` through
  `harness::function::trigger`, and captures the result as a `function_result` message.

The dispatch policy is **fail-closed**: a call is dispatched only if the target matches an `allow`
glob and no `deny` glob (see `harness::send` options). When `options.functions` is omitted entirely,
every call is denied with an `is_error` function_result explaining the policy — a default install is
a plain chat loop until functions are explicitly allowed. The globs are structural and final: hooks
run only *after* they pass, so an approval sibling cannot intercept a no-match denial — a deployment
that wants every call human-gated instead allows broadly (`allow: ["*"]`) and lets the
[approval-gate](approval-gate.md) hook hold or deny per its policy (see
[Out of scope](#out-of-scope-future-sibling-workers)).

`engine::functions::list` is how the **model** discovers what's callable — by triggering it through
`agent_trigger` at runtime — not how the harness builds a schema list at turn start. The harness
post-filters `engine::functions::list` / `engine::functions::info` results through the same
allow/deny globs before folding them into the `function_result`, so the model only discovers
functions it can actually call.

**Exposure modes.** `options.functions.expose` selects how allowed functions reach the model:

- `"agent_trigger"` (default) — the single generic schema above. Zero per-function schema tokens;
  discovery happens at runtime.
- `"native"` — at turn start the harness expands the `allow` globs against the registry
  (`engine::functions::list` / `engine::functions::info`) and attaches one provider tool schema per
  allowed function. Models follow concrete per-function schemas more reliably than a generic
  `{ function, payload }` wrapper, and no turns are spent on discovery — the right mode for narrow
  agents (sub-agents especially) with small fixed toolsets. Costs registry reads at turn start and
  schema tokens per function, so keep the allow-list tight.

Both modes enforce the same fail-closed allow/deny policy at dispatch time; `expose` changes only
what the model sees. The synthetic `submit_result` schema (see [Output contract](#output-contract))
is harness-internal in either mode and is never dispatched through `iii.trigger`.

A function can do anything an iii function can, including calling back to the consumer (the diagram's
`funcs -> chat` edge) — that is the function's own behaviour, not the harness's.

Terminology: see [README.md § Terminology](README.md#terminology).

## Deferred trigger (pending function results)

Some calls cannot resolve inside a dispatch: a sub-agent that runs for minutes, or an approval that
waits for a human. Holding the queue step open would break the durability contract (the visibility
timeout must exceed the worst-case step duration) and would serialise independent work. Dispatch
may therefore defer:

- `harness::function::trigger` may return `{ pending: true }` instead of a result. The dispatch
  loop checkpoints the call as `state: "pending"` and continues with the remaining calls in the
  message.
- When the trigger pass ends with unresolved pending calls, the turn **parks**: the step ends
  *without* re-enqueueing `harness::turn`, the turn stays `awaiting_functions`, and **no queue step
  is held** while the deferred work runs — pending calls never stretch the visibility-timeout
  bound.
- [`harness::function::resolve`](#harnessfunctionresolve) settles the call later — either
  **delivering** a result (appended under the same deterministic entry id a direct trigger would
  have used, `e_<turn_id>_<function_call_id>`, checkpoint flipped to `done`) or, for hook-held
  calls, **releasing** it for execution (`action: "execute"` — on resume the loop runs the call
  through the remaining trigger pipeline). When it settled the last pending call — or a release
  needs the loop — it re-enqueues `harness::turn` so the model reacts. Duplicate resolves hit the
  existing entry id and are no-ops.
- Every pending call carries a `pending_timeout_ms` (default 30 minutes). A periodic sweep resolves
  expired calls with `is_error: true` (`"pending call timed out"`), so a lost child or an abandoned
  approval can never park a turn forever. A hold's owner typically settles expiry itself first with
  a richer denial (see [approval-gate.md](approval-gate.md#sweep-the-gc-backstop)); this sweep is
  the backstop when no owner is alive — double resolution is a no-op either way.
- Steering still works while parked: user messages appended in the meantime sit after the
  watermark and fold in on resume (see [Concurrency & steering](#concurrency--steering)).

[`harness::spawn`](#sub-agents-harnessspawn) is the built-in pending trigger. The mechanism is
deliberately general: an approval sibling implements *hold* by returning `hold` from a
pre-trigger hook and calling `harness::function::resolve` on the human decision (`execute` to
release the call, `deliver` to answer it with a denial) — no new loop machinery (see
[approval-gate.md](approval-gate.md)).

## Sub-agents (`harness::spawn`)

A sub-agent is an ordinary harness run in a **child session**, spawned as a function call from a
parent turn. Each child gets its own goal (the spawn `task`), its own policy, and — via the
[output contract](#output-contract) — its own typed deliverable: free text, JSON, or
schema-validated JSON. The model calls `harness::spawn` through `agent_trigger`; the dispatch
reports the call **pending**, the parent parks, the child runs its own turns on the `harness-turn`
queue (parallel across sessions by construction — spawn three children and they run concurrently),
and the child's completion resolves the parent's call with the child's result.

The spawn dispatch, step by step:

1. **Guards** — violations fail the call with an `is_error` function_result, never a throw:
   `harness::spawn` must match the parent's allow globs (fail-closed, like any target);
   `depth + 1 > max_depth` (default 3) is refused (`harness/spawn_depth_exceeded`); non-terminal
   children of this turn at or above `max_children` (default 5) is refused
   (`harness/spawn_fanout_exceeded`).
2. **Policy subsetting.** The child's function policy is the requested one **intersected with the
   parent's**: a child `allow` matches only what the parent's `allow` also matches, and parent
   `deny` globs are inherited. A child can narrow, never escalate. The child's `max_turns` is
   capped at the parent's remaining turn budget.
3. **Create the child session** with linkage metadata merged into `SessionMeta.metadata` —
   `{ parent_session_id, parent_turn_id, function_call_id, depth }` — so UIs reconstruct the tree
   and trigger configs can filter on it (see
   [session-manager.md § Sub-agent linkage](session-manager.md#sub-agent-linkage)).
4. **CAS-create the child turn** (the same seed path as `harness::send`) with the subset policy,
   `depth = parent.depth + 1`, the `task` as the opening user message, and the requested output
   contract. Record `calls[call_id] = { state: "pending", child_session_id, child_turn_id }` on the
   parent turn record and report pending — the parent parks.
5. **Child completion.** The child's finalise step (any terminal status) reads the parent linkage
   off its turn record and calls `harness::function::resolve` on the parent: `completed` delivers
   the child's result (structured result in `details`, text rendering in `content`);
   `failed` / `cancelled` deliver `is_error: true` with the reason. The deterministic entry id
   makes a redelivered child step unable to double-resolve.

**Cancellation cascade.** `harness::stop` on the parent walks `calls` for non-terminal children and
stops each child session, recursively. A child stopped this way resolves its parent call with
`is_error: true` (`"cancelled"`).

**Context inheritance.** Fresh by default: the child sees only its `system_prompt` and `task`. A
caller that wants parent history can `session::fork` first and spawn into the fork
(`SpawnRequest.session_id`) — supported, not default (inherited transcripts inflate child context
and cost).

**What spawn is not.** Not agent-to-agent messaging, not a workflow engine, not a shared
blackboard — those compose on top as siblings (see
[Out of scope](#out-of-scope-future-sibling-workers)). One parent turn fans out to bounded children
and joins on their results; that is the whole feature.

## Output contract

A turn can declare what it must produce — free text (default) or JSON, optionally validated against
a JSON Schema. This is what turns a sub-agent or a backend [`harness::send`](#harnesssend) call into a
typed unit of work instead of "parse the transcript yourself". The shape (`OutputContract`) is
shared — see [README § Output contract](README.md#output-contract).

- `{ type: "text" }` (default) — the result is the final assistant message's text; nothing is
  enforced.
- `{ type: "json", schema? }` — the result is a JSON value, validated against `schema` when
  supplied. The harness picks a strategy per turn:
  - **Provider-native** — when `router::models::supports(model, "structured_output")` is true, the
    harness passes `response_format: { type: "json", schema }` on
    [`router::chat`](llm-router.md#routerchat) and parses the final assistant text as the result.
  - **`submit_result` fallback** — otherwise the harness injects a synthetic `submit_result`
    invocation schema (its `parameters` are the output schema) alongside the normal exposure mode.
    The model ends the job by calling it; the harness validates the arguments, records them as the
    turn result, and finalises — the call is consumed by the harness, never dispatched through
    `iii.trigger`. `submit_result` is terminal: other calls in the same message trigger first
    (their results still land in the transcript), then the turn finalises.
  - **Validation retries** — if the model stops without a valid result (no `submit_result`, schema
    mismatch, unparseable JSON), the harness appends a synthetic user nudge carrying the validation
    errors and re-enqueues a generate step, at most `max_validation_retries` times (default 2).
    After that the turn ends `completed` with `result_error` set and the raw final text as a
    best-effort `result`.

The result is stored on the turn record, returned by [`harness::status`](#harnessstatus), carried on the
[`harness::turn_completed`](#trigger-types-emitted) event, and — for sub-agents — delivered to the
parent in the `function_result` (`details` carries the structured value; `content` a text
rendering).

## Hooks

Hooks are the **synchronous** counterpart to the turn events: iii functions the harness calls
*in-path* at fixed points of the loop, which can veto, hold, or mutate what happens next. The rule
for choosing between them: if you only need to *know*, bind an event
([`harness::turn_started` / `turn_completed`](#trigger-types-emitted), or the session triggers); if
you must *block or change* something, bind a hook. Every hook adds latency and a failure mode
to the hot path — events are always the cheaper tool.

### Hook points

| Point | Runs | May do |
|---|---|---|
| `pre_turn` | first step of a turn, after `working` is set, before any model spend | veto (`deny` ends the turn with the reason) |
| `pre_generate` | after `context::assemble`, before `router::chat` | replace/extend `system_prompt`; **append-only** message injection; veto |
| `post_generate` | after the final `AssistantMessage` update of a generate step | observe only (usage accounting, safety logging) |
| `pre_trigger` | after the fail-closed glob policy passes, before the target is invoked | `deny` (is_error result), `hold` (park via deferred trigger), rewrite `arguments` |
| `post_trigger` | after the target returns, before the `function_result` is appended | rewrite result `content` / `details` / `is_error` (redaction, truncation) |

The asymmetries are deliberate:

- `post_generate` cannot mutate — the message already streamed to consumers
  (`session::message-updated` fired per delta); redacting after the fact would lie to the UI. Strip
  secrets where they *enter* instead: `post_trigger` runs before the result is persisted, so the
  transcript and the model both see the rewritten version.
- `pre_generate` injection is **append-only** (plus the system prompt): rewriting history would
  break the call/result pairing invariants `context::assemble` guarantees (see
  [context-manager.md § Structural invariants](context-manager.md#structural-invariants)) and
  invalidate provider prompt caches. Injected content rides the `reserved_tokens` headroom of the
  token budget (default `min(20000, 10% of context_window)` — see
  [context-manager.md § Token budget model](context-manager.md#token-budget-model)); keep it small
  and bounded.
- `pre_trigger` runs **after** the allow/deny globs: a hook can narrow the policy, never bypass it.

### Registration

Hooks are **iii triggers**. The harness registers one custom trigger type per hook point —
`harness::hook::pre_turn`, `harness::hook::pre_generate`, `harness::hook::post_generate`,
`harness::hook::pre_trigger`, `harness::hook::post_trigger` — alongside its async
[turn events](#trigger-types-emitted), and a sibling binds a hook with the standard two-step
pattern (see [README § Reactive pattern](README.md#reactive-pattern)): register the hook function,
then register a trigger of the point's type. A sibling wires itself at startup — installing the
worker is installing the hook; there is no hook block in any configuration entry.

```typescript
iii.registerFunction("approval::gate", gateHandler);
iii.registerTrigger({
  type: "harness::hook::pre_trigger",
  function_id: "approval::gate",
  config: { functions: ["shell::*", "harness::spawn"], timeout_ms: 5000 },
});

iii.registerTrigger({
  type: "harness::hook::post_trigger",
  function_id: "redactor::scrub",
  config: {},
});
iii.registerTrigger({
  type: "harness::hook::post_generate",
  function_id: "budget::record",
  config: { on_error: "fail_open" },
});
```

Unlike the turn events, delivery is **synchronous and result-bearing**: the harness owns these
trigger types (see [README § Trigger delivery](README.md#trigger-delivery)) and invokes each bound
function *in-path* via `iii.trigger`, treating the return value as a [`HookOutput`](#contract).
Trigger registration is only the binding surface; the delivery semantics are the
[chain semantics](#chain-hold-and-failure-semantics) below, not async fan-out.

There is still no per-send hook injection, and the model cannot bind hooks — trigger registration
is a worker/SDK surface, never an iii function reachable through `agent_trigger`. The effective
set stays auditable: `engine::triggers::list` enumerates every binding, and the harness rebuilds
its subscriber set from the engine after a restart. A binding whose worker is dead or slow cannot
brick the loop silently: every hook invocation is bounded by its `timeout_ms` and resolved by its
`on_error` policy (a fail-closed `pre_*` hook denies rather than waving through — see
[failure semantics](#chain-hold-and-failure-semantics)); unbinding is `unregisterTrigger`.

### Contract

```typescript
type HookPoint = "pre_turn" | "pre_generate" | "post_generate" | "pre_trigger" | "post_trigger";

// the `config` of a `harness::hook::<point>` trigger binding;
// the binding's function_id is the hook the harness calls (sync)
type HookTriggerConfig = {
  functions?: string[];               // pre/post_trigger only: target function_id globs
  priority?: number;                  // chain order: ascending, ties by function_id (default 0)
  timeout_ms?: number;                // default 5_000
  on_error?: "fail_closed" | "fail_open"; // default: fail_closed for pre_*, fail_open for post_*
};

type HookInput = {
  point: HookPoint;
  session_id: string;
  turn_id: string;
  step: number;
  depth: number;                       // sub-agent depth (hooks run for child turns too)
  metadata?: Record<string, unknown>;  // the per-send tracing metadata
  // point-specific payload (pre_turn carries only the envelope):
  generate?: { system_prompt: string; messages: AgentMessage[]; model: string; provider: string };
  generated?: { message: AssistantMessage };                       // post_generate (read-only)
  call?: { id: string; function_id: string; arguments: unknown };  // pre_trigger
  result?: { function_call_id: string; function_id: string;        // post_trigger
             content: ContentBlock[]; is_error: boolean; details?: unknown };
};

type HookOutput =
  | { decision: "continue";
      mutations?: {
        system_prompt?: string;           // pre_generate
        append_messages?: AgentMessage[]; // pre_generate (appended after the candidate window)
        arguments?: unknown;              // pre_trigger
        content?: ContentBlock[];         // post_trigger
        details?: unknown;                // post_trigger
        is_error?: boolean;               // post_trigger
      };
      annotations?: Record<string, unknown> } // merged into the written entry's origin (audit trail)
  | { decision: "deny"; reason: string }      // pre_turn / pre_generate / pre_trigger
  | { decision: "hold"; pending_timeout_ms?: number } // pre_trigger only
  | void;                                     // = continue, no changes
```

### Chain, hold, and failure semantics

- **Middleware chain.** Hooks for a point run in deterministic order — ascending `priority`
  (default 0), ties broken by `function_id` — each receiving the payload as mutated by the
  previous one; the first `deny` or `hold` short-circuits the rest. Mutations from different hooks
  can conflict — keep chains short and set `priority` deliberately.
- **Deny.** At `pre_turn` / `pre_generate` the turn ends `failed` with the hook's `reason` (a
  `custom` error entry + `harness::turn_completed`, like any failure). At `pre_trigger` the call
  is answered with an `is_error` function_result carrying the reason — the model sees it and can
  adapt.
- **Hold** (`pre_trigger` only) reuses
  [deferred trigger](#deferred-trigger-pending-function-results) wholesale: the call checkpoints
  as `pending` with `held_by: <hook function_id>`, the turn parks, and whoever owns the decision
  calls [`harness::function::resolve`](#harnessfunctionresolve) — `action: "execute"` to release
  the call through the remaining trigger pipeline, or `deliver` to answer it (e.g. a denial). The
  pending sweep timeout still applies. An approval gate is exactly this: a `pre_trigger` hook that
  returns `hold`, a decision UI, and a `resolve` call — no loop changes (see
  [approval-gate.md](approval-gate.md)).
- **Failure policy.** A hook that throws, times out (`timeout_ms`, default 5s), or is unavailable
  is resolved by its `on_error`: `fail_closed` treats it as `deny` (default for `pre_*` — a crashed
  approval hook must not wave calls through); `fail_open` skips it (default for `post_*` — a
  logging outage must not kill the agent).
- **Replays.** Hooks run inside at-least-once steps: a redelivered step re-runs its hooks. Hooks
  MUST be idempotent; key side effects on `turn_id + step` or `function_call_id`.
- **Provenance.** Hook invocations carry the agent provenance mark of the work they wrap, and the
  engine propagates it through any nested triggers the hook makes — a hook cannot launder an
  agent-gated call (see [README § Security model](README.md#security-model)).

### Cautions

For operators wiring hooks and developers writing them:

1. **You are on the hot path.** Hook time extends step duration, which the queue's visibility
   timeout must cover (see [Durability & idempotency](#durability--idempotency)) — and a spawn
   tree pays your chain on every child step. Keep hooks fast; never wait for a human inline —
   return `hold`.
2. **`on_error` is a security decision.** Fail-closed on an observability hook turns a logging
   outage into a dead agent; fail-open on a policy hook turns an approval outage into a bypass.
   The defaults encode the safe choice per point — change them knowingly.
3. **Treat hook input as untrusted.** `arguments`, generated text, and function output are
   model-controlled. Never execute or forward them blindly from hook code — the hook runs with
   worker privileges and is a textbook confused-deputy target.
4. **Mutations are silent.** The transcript stores the *effective* (rewritten) values. Return
   `annotations` (merged into the entry's `origin`) or log originals yourself, or audits will show
   data that never matched what actually ran.
5. **Don't start turns from hooks.** A hook calling `harness::send` can loop
   (hook -> turn -> hook). If a hook must trigger follow-up work, emit through a queue or carry a
   hop counter in `session.metadata`.
6. **Reach for events first.** If observe-only is enough, bind
   [`harness::turn_completed`](#trigger-types-emitted) or the session triggers instead — hooks are
   for the cases that must block or change the loop.

## Registered functions

- `harness::send` — Entry point: persist the incoming message and kick off a turn; returns fast.
- `harness::spawn` — Spawn a sub-agent in a child session; the model-facing pending trigger (see
  [Sub-agents](#sub-agents-harnessspawn)).
- `harness::turn` — Internal durable loop step (enqueued); not called directly by consumers.
- `harness::function::trigger` — Internal: unwrap an `agent_trigger` call and invoke the target iii
  function; capture its result — or report it `pending`.
- `harness::function::resolve` — Internal: deliver a pending call's result and resume the parked
  turn.
- `harness::stop` — Request cancellation of an in-flight turn (cascades to spawned children).
- `harness::status` — Read the current turn status for a session.

## Agent exposure

Deny-by-default for in-run agents (see [README § Security model](README.md#security-model)):

- **Deny:** `harness::send` — self-invocation: a model that can start arbitrary
  turns can fork unbounded loops outside any `max_turns` guard; `harness::turn` (internal);
  `harness::function::trigger`
  (forged call ids, policy re-entry); `harness::function::resolve` (forged results for parked
  calls); `harness::stop`.
- **Allow (gated):** `harness::spawn` — the controlled alternative to `send`: the harness itself
  enforces depth / fan-out / turn budgets and policy subsetting (see
  [Sub-agents](#sub-agents-harnessspawn)), and the fail-closed dispatch policy still applies — a
  deployment turns it on per agent by adding `harness::spawn` to the `allow` globs.
- **Safe:** `harness::status` (read-only).

## Triggers

### Trigger types emitted

Session events remain the rendering surface (live transcripts, spinners); these two types are the
**orchestration surface** — they fire at turn boundaries so consumers and siblings react without
polling `harness::status`. Events are async and observe-only; a sibling that must *block or
mutate* the loop binds a [hook](#hooks) instead. Bind with the standard two-step pattern (see
[README § Reactive pattern](README.md#reactive-pattern)).

- **`harness::turn_started`** — a turn began executing (first loop step).
  - Config: `{ session_id?: string; parent_session_id?: string }`.
  - Payload:

```typescript
type TurnStartedEvent = {
  session_id: string;
  turn_id: string;
  parent?: { session_id: string; turn_id: string; function_call_id: string }; // sub-agent turns only
  timestamp: number;
};
```

- **`harness::turn_completed`** — a turn reached a terminal status.
  - Config: `{ session_id?: string; parent_session_id?: string }`.
  - Payload:

```typescript
type TurnCompletedEvent = {
  session_id: string;
  turn_id: string;
  status: "completed" | "cancelled" | "failed";
  result?: unknown;        // output-contract result (see Output contract)
  result_error?: string;   // set when the contract could not be satisfied
  reason?: string;         // failure cause when status is "failed"
  parent?: { session_id: string; turn_id: string; function_call_id: string };
  timestamp: number;
};
```

A backend worker that chains agents binds `harness::turn_completed` and calls `harness::send`
from the handler — that is the supported way to build event-driven loops. **The
loop guard is the consumer's:** `max_turns` bounds one turn, not a chain of turns; an event loop
(completed -> send -> completed -> …) must carry its own termination condition — a hop counter in
`session.metadata`, a budget sibling, or a terminal check in the handler.

### Hook trigger types (synchronous)

The five `harness::hook::*` types — `pre_turn`, `pre_generate`, `post_generate`, `pre_trigger`,
`post_trigger` — are registered trigger types too, but binding one puts the function **in-path**:
the harness invokes it synchronously at the hook point, in `priority` order, and acts on its
return value (veto / hold / mutate), under the per-binding timeout and `on_error` policy. Config
shape, chain order, and failure semantics are specified in [Hooks](#hooks); the async types above
remain the right surface for anything observe-only.

### Triggers bound

- **Optional** `harness::on_steering` — bind to [`session::message-added`](session-manager.md#trigger-types-emitted)
  so a user message that arrives mid-turn is folded into the running turn rather than dropped. If a
  turn is already running, the loop's steering check (step 6) picks the new message up on its own; if
  none is running, the handler kicks a fresh turn:

```typescript
iii.registerFunction("harness::on_steering", async (evt) => {
  if (evt.message.role !== "user") return;
  const status = await iii.trigger({
    function_id: "harness::status",
    payload: { session_id: evt.session_id },
  });
  if (
    !status ||
    status.status === "completed" ||
    status.status === "cancelled" ||
    status.status === "failed"
  ) {
    // model/options for the fresh turn come from app config — a merged send ignores them anyway.
    await iii.trigger({
      function_id: "harness::send",
      payload: { session_id: evt.session_id, message: evt.message, model: "<model>" },
    });
  }
  // else: a turn is running; its steering check (watermark) folds this message in.
});

iii.registerTrigger({
  type: "session::message-added",
  function_id: "harness::on_steering",
  config: { roles: ["user"] },
});
```

This is opt-in; the default harness binds no triggers. Prefer routing inbound messages through
`harness::send` rather than raw `session::append` where possible — the merge path double-checks the
turn record after appending (see [Concurrency & steering](#concurrency--steering)), closing the
read/complete race this handler otherwise has between `harness::status` and `harness::send`.

---

## API Reference

Shared types (`AgentMessage`, `ContentBlock`, `AssistantMessage`, `AgentFunction`, `ThinkingLevel`,
`OutputContract`) are defined in
[README.md § Cross-cutting contracts](README.md#cross-cutting-contracts).

```typescript
type TurnStatus =
  | "running"              // generating or between durable steps
  | "awaiting_functions"   // triggering function calls / parked on pending results
  | "completed"            // turn finished normally (incl. max_turns cap)
  | "cancelled"            // harness::stop observed
  | "failed";              // unexpected error; turn record carries reason
```

### `harness::send`

Accept an incoming message, ensure the session, append the user message, and enqueue the first turn
step. Returns before the turn runs. If a turn is already running for the session, the message is
appended and folded into it instead — no second turn starts (see
[Concurrency & steering](#concurrency--steering)).

**Idempotency.** Webhook sources redeliver (Telegram updates, Slack retries). When
`idempotency_key` is set, the user entry id derives from it (the duplicate append is a no-op) and
the key maps to the `{ session_id, turn_id }` it first produced (see [State](#state), TTL-bound);
a redelivered send returns the same response with `deduplicated: true` and changes nothing.

**Sessions.** `session.metadata` lands on `SessionMeta.metadata` when the send creates/ensures the
session — the tenancy hook session trigger configs and `session::list` filter on.

- Invocation: **sync** (kicks an async/enqueued loop)

Request:

```typescript
type SendRequest = {
  session_id?: string;            // omit to create a new session
  message: AgentMessage | string; // string is sugar for a user text message;
                                  // role must be "user" or "custom" (else harness/invalid_message_role).
                                  // "custom" content never reaches the model (no wire mapping) —
                                  // such a send kicks a turn over the existing history only
  model: string;
  provider?: string;
  idempotency_key?: string;       // webhook dedupe: a repeated key returns the original
                                  // {session_id, turn_id} and appends nothing
  session?: {                     // applied when this send creates/ensures the session
    title?: string;
    metadata?: Record<string, unknown>; // -> SessionMeta.metadata (the tenancy hook)
  };
  options?: {
    system_prompt?: string;
    max_turns?: number;           // default 16
    thinking_level?: ThinkingLevel;
    output?: OutputContract;      // the turn's deliverable; default { type: "text" } (see Output contract)
    functions?: {
      allow?: string[];           // function_id globs the agent may dispatch to (e.g. "shell::*")
      deny?: string[];
      expose?: "agent_trigger" | "native"; // default "agent_trigger" (see Functions (the white box))
    };
    metadata?: Record<string, unknown>; // tracing passthrough (session_id/message_id propagate)
  };
};
```

Response:

```typescript
type SendResponse = {
  session_id: string;
  turn_id: string;     // the new turn — or the running turn when merged
  accepted: true;
  merged?: boolean;    // true when folded into an in-flight turn (steering)
  deduplicated?: boolean; // true when idempotency_key matched an earlier send
};
```

Example:

```jsonc
// request
{ "message": "Summarise the repo README", "model": "claude-sonnet-4", "provider": "anthropic",
  "options": { "functions": { "allow": ["shell::*", "fs::*"] } } }
// response
{ "session_id": "s_7a1", "turn_id": "t_001", "accepted": true }
```

### `harness::spawn`

Spawn a sub-agent in a child session (see [Sub-agents](#sub-agents-harnessspawn)). Designed to be
called **by the model** through `agent_trigger` — the controlled, guarded alternative to exposing
`harness::send`. Triggered from a turn, the trigger layer records the child linkage and reports
the call `pending`; the child's result arrives as the call's `function_result` when it finishes.
(Called directly by a consumer, it simply starts a linked child and returns — `send` is
usually what consumers want.)

- Invocation: **sync** (kicks the child's enqueued loop; the *call result* is deferred)

```typescript
type SpawnRequest = {
  task: string | AgentMessage;    // the child's goal — its opening user message
  model?: string;                 // defaults to the parent turn's model
  provider?: string;
  session_id?: string;            // spawn into an existing session (e.g. a fork); default: create fresh
  options?: {
    system_prompt?: string;
    max_turns?: number;           // capped at the parent's remaining turn budget
    thinking_level?: ThinkingLevel;
    output?: OutputContract;      // the child's deliverable: text / json / json+schema
    functions?: {                 // intersected with the parent policy — narrow, never escalate
      allow?: string[];
      deny?: string[];
      expose?: "agent_trigger" | "native";
    };
    max_children?: number;        // fan-out guard for the child's own spawns (default 5)
    pending_timeout_ms?: number;  // parent-side wait guard for this child (default 1_800_000)
  };
  // Parent linkage (parent_session_id / parent_turn_id / function_call_id / depth) is injected by
  // the triggering harness from the turn record — never trusted from model-supplied arguments.
};
type SpawnResponse = {
  child_session_id: string;
  child_turn_id: string;
};
```

Guard failures surface as `is_error` function_results, not throws: `harness/spawn_depth_exceeded`,
`harness/spawn_fanout_exceeded`, or the standard policy denial when `harness::spawn` is not
allowed. The child's transcript is a normal session — bind `session::message-updated` with
`{ metadata: { parent_session_id } }` to render sub-agent progress live.

### `harness::turn`

Internal durable loop step. Documented for completeness; consumers do not call it. Enqueued onto the
`harness-turn` queue (FIFO per session, parallel across sessions — see
[Dependencies](#dependencies)); each run advances one step of [the loop](#the-loop).

- Invocation: **enqueue** (`TriggerAction.Enqueue({ queue: "harness-turn" })`)

```typescript
type TurnStepPayload = {
  session_id: string;
  turn_id: string;
  step: number;          // monotonic; guards against stale/duplicate dequeues
};
type TurnStepResult = {
  session_id: string;
  status: TurnStatus;
  next_step?: number;    // present while the loop continues
};
```

Failure handling: an unexpected throw marks the turn `failed`, appends a `custom`
(`custom_type: "error"`) entry so the UI sees the reason, sets `session::set-status error` with a
short `reason`, and emits [`harness::turn_completed`](#trigger-types-emitted)
(`status: "failed"`) — resolving the parent's pending call with `is_error: true` when the turn is a
sub-agent (see [Sub-agents](#sub-agents-harnessspawn)). A step may opt into queue retry/backoff for
transient provider errors instead of failing the turn (subject to
[Durability & idempotency](#durability--idempotency)).

### `harness::function::trigger`

Invoke a single iii function and return a normalised result. The loop unwraps `agent_trigger` before
calling this; it can also be called directly with an already-unwrapped target `function_id` +
`arguments`.

- Invocation: **sync**

```typescript
type FunctionDispatchRequest = {
  session_id: string;
  call: {
    id: string;            // function_call id, echoed into the result
    function_id: string;   // the iii function to invoke
    arguments: unknown;
  };
};
type FunctionDispatchResponse =
  | {
      function_call_id: string;
      function_id: string;
      content: ContentBlock[]; // function output, normalised to content blocks
      is_error: boolean;
      details?: unknown;
      duration_ms: number;
    }
  | {
      function_call_id: string;
      function_id: string;
      pending: true;           // result arrives later via harness::function::resolve
      pending_timeout_ms?: number;
    };
```

Dispatch is a pipeline: the fail-closed allow/deny globs first, then the `pre_trigger`
[hook chain](#hooks) (deny -> `is_error` result; hold -> `pending`; argument rewrite), then the
target invocation, then the `post_trigger` chain over the result (redaction, truncation) before it
is returned and appended. Policy denials and the synthetic "interrupted" results from redelivery
skip `post_trigger` — nothing executed. A trigger therefore either returns the (possibly
rewritten) result inline or reports the call **pending** (see
[Deferred trigger](#deferred-trigger-pending-function-results)): `harness::spawn` is the built-in
pending trigger, and a `pre_trigger` hook returning `hold` is the pluggable one — an approval
gate is that hook plus a `harness::function::resolve` call on the decision (see
[approval-gate.md](approval-gate.md)).

### `harness::function::resolve`

Settle a `pending` call and resume the parked turn (see
[Deferred trigger](#deferred-trigger-pending-function-results)). Called by the harness itself
(child completion) or by a trusted sibling (an approval decision — see
[approval-gate.md](approval-gate.md)); denied to in-run agents. Two actions:

- **`deliver`** (default) — the caller supplies the call's result: the harness appends the
  `function_result` under the deterministic entry id and flips the checkpoint to `done`. This is
  how child completions, approval denials, and timeout sweeps settle a call.
- **`execute`** — valid only for calls held by a `pre_trigger` hook (`held_by` set): the caller
  supplies no result; the harness marks the call *released* and re-enqueues the turn, and the loop
  step runs the call through the **remaining trigger pipeline** — the `pre_trigger` chain
  resumes after the holding hook (the glob policy and earlier hooks already passed), the target is
  invoked with the original call's provenance, the `post_trigger` chain runs over the result, and
  the result is appended as if the call had never been held. The checkpoint flips
  `pending → triggered → done`, so the at-most-once redelivery rule applies unchanged (see
  [Durability & idempotency](#durability--idempotency)). This is how an approval *allow* releases
  a held call without the approver re-implementing trigger — invoking the target itself would
  bypass `post_trigger` redaction, the per-call checkpoints, and provenance.

Idempotent in both modes: `deliver` writes the deterministic entry id
(`e_<turn_id>_<function_call_id>`), so duplicates change nothing; a duplicate `execute` finds the
checkpoint no longer `pending` and returns `resolved: false`.

- Invocation: **sync**

```typescript
type FunctionResolveRequest = {
  session_id: string;
  turn_id: string;
  function_call_id: string;
  action?: "deliver" | "execute"; // default "deliver"; "execute" only for held calls (held_by set)
  // deliver only (required then; ignored on execute):
  content?: ContentBlock[];
  is_error?: boolean;
  details?: unknown;          // structured payload (e.g. a child's output-contract result)
};
type FunctionResolveResponse = {
  resolved: boolean;          // false when the call is unknown, already done, or (execute) not held
  turn_resumed: boolean;      // true when this resolve re-enqueued the turn
                              // (deliver: last pending call settled; execute: always)
};
```

### `harness::stop`

Request cancellation. Sets an abort flag the next `harness::turn` step observes, and aborts an
in-flight stream via [`router::abort`](llm-router.md#routerabort) using the `stream_request_id` on
the turn record. Non-terminal spawned children recorded in `calls` are stopped first, recursively —
each resolves its parent call with `is_error: true` (see [Sub-agents](#sub-agents-harnessspawn)).
The turn record transitions to `cancelled` before `session::set-status done`, and
[`harness::turn_completed`](#trigger-types-emitted) fires with `status: "cancelled"`.

- Invocation: **sync**

```typescript
type StopRequest = { session_id: string; turn_id?: string }; // turn_id omitted = current turn
type StopResponse = { stopping: boolean };
```

### `harness::status`

- Invocation: **sync**

```typescript
type StatusRequest = { session_id: string };
type StatusResponse = {
  session_id: string;
  turn_id: string | null;
  status: TurnStatus;
  step: number;
  turn_count: number;
  max_turns: number;
  depth: number;                      // 0 for top-level turns; >0 for sub-agents
  pending_function_calls: string[];   // function_call ids awaiting results (incl. parked children)
  children: Array<{                   // live spawned children of the current turn
    function_call_id: string;
    session_id: string;
    turn_id: string;
  }>;
  result?: unknown;                   // output-contract result (terminal turns)
  result_error?: string;
} | null;                          // null for unknown sessions
```

---

## State

| Scope | Key | Value | Purpose |
|---|---|---|---|
| `harness_turn` | `<session_id>` | turn record `{ turn_id, status, step, turn_count, depth, abort?, watermark_entry_id?, stream_request_id?, options, calls, parent?, result?, result_error? }` | Loop progress, per-send options (incl. output contract), per-call checkpoints `(`triggered` / `pending` / `done` + child linkage + `held_by` for [hook](#hooks) holds), steering watermark, sub-agent linkage, turn result; survives restart. Seeded by CAS from `harness::send` / `harness::spawn` (see [Concurrency & steering](#concurrency--steering)). |
| `harness_idem` | `<idempotency_key>` | `{ session_id, turn_id, entry_id, ts }` | `harness::send` webhook dedupe (TTL ~24h). |

Transcript truth lives in [session-manager](session-manager.md); the harness keeps only loop
bookkeeping. Neither scope expires on its own: `harness_idem` rows are TTL-bound by contract, and a
deployment that deletes sessions can purge the corresponding `harness_turn/<session_id>` record
from a [`session::deleted`](session-manager.md#trigger-types-emitted) binding — the same cascade
pattern [approval-gate](approval-gate.md#state-lifecycle) mandates for its own scopes.

## Dependencies

- `session-manager` (`session::*`) — persist messages, stream content via `session::update-message`,
  and set status. Required.
- `llm-router` (`router::chat`) — generation. Required.
- `context-manager` (`context::assemble`) — context budgeting. Soft; degrades to raw history.
- `iii-queue` — the durable `harness-turn` loop. The queue MUST provide per-session ordering with
  cross-session parallelism (partition by `session_id`); a single global FIFO would head-of-line
  block every session behind one long stream step. Sub-agent turns are ordinary entries on this
  queue under their child `session_id` — which is what makes spawned children run in parallel.
- iii engine — `iii.trigger` for function dispatch (`agent_trigger` unwrap → target function);
  registry reads (`engine::functions::list` / `engine::functions::info`) for runtime discovery and
  `expose: "native"` schema mapping; custom trigger-type registration (`registerTriggerType`) for
  the [turn events](#trigger-types-emitted) and the [hook points](#hooks), with subscriber sets
  rebuilt from the engine after a restart.

## Out of scope (future sibling workers)

Kept out to preserve thinness; each is a clean add-on that wraps the loop or subscribes to its events:

- **approval-gate** — the *policy and decision surface* for holding function calls: which calls
  need a human, who may approve, where decisions land. Specified in
  [approval-gate.md](approval-gate.md). The mechanics are already in core — a `pre_trigger`
  [hook](#hooks) returning `hold` plus
  [`harness::function::resolve`](#harnessfunctionresolve) on the decision (`execute` to release,
  `deliver` to deny); the sibling ships the hook function and its trigger binding, the pending
  inbox and its triggers, and the UI — never loop changes.
- **llm-budget** — track spend from `router` usage and cap per workspace/agent: a `pre_turn` /
  `pre_generate` [hook](#hooks) enforces the cap, and
  [`harness::turn_completed`](#trigger-types-emitted) plus the sub-agent linkage metadata give it
  per-tree aggregation.
- **context-scheduler** — decide *when* to compact (the optional reactive trigger in
  [context-manager](context-manager.md#triggers)); the harness only compacts inline on overflow.
- **orchestration beyond spawn/join** — agent-to-agent messaging, shared blackboards, workflow
  graphs, supervisor pools. The harness ships [`harness::spawn`](#sub-agents-harnessspawn) (bounded
  fan-out/join) only; richer patterns compose on top of spawn and the turn events.
- **agent presets** — named bundles of per-send options (model, system prompt, function policy,
  output contract) shared across consumer surfaces. Consumers own their per-send options; a preset
  sibling can resolve a name into a `SendRequest` and call `harness::send` itself — the harness
  resolves nothing.

(The previously planned **hook-fanout** sibling is superseded: synchronous lifecycle interception
is the core [Hooks](#hooks) surface, and async observation is the turn events.) Siblings that need
to sit *inside* the critical path bind a [hook](#hooks); everything else binds events.

## Boundaries

- Does **not** store the transcript, build context, or talk to providers itself — it calls the other
  three.
- Does **not** gate approvals, meter cost, or schedule compaction in v1 — it provides the
  [hook points](#hooks) and the
  [deferred trigger](#deferred-trigger-pending-function-results) primitive; the policy logic
  (who approves, how much to spend) always lives in the sibling that registers the hook.
- Does **not** orchestrate beyond spawn/join: `harness::spawn` bounds the multi-agent surface to
  child sessions that resolve a single parent call; messaging, blackboards, and workflow graphs are
  siblings.
- Does **not** define functions — functions are the iii substrate; the harness only exposes the
  `agent_trigger` / native invocation surfaces and dispatches what the model requests
  (`submit_result` being the one synthetic, never-dispatched exception).
