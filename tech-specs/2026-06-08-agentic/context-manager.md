# context-manager

Worker prefix: `context::*`

## Definition

`context-manager` turns a raw conversation history plus a target model into a **model-ready
context**: a system prompt and an ordered `AgentMessage[]` that fits inside the model's usable token
budget. It owns the policy for *what the model sees this turn* — token counting, function-result
pruning, and history compaction (summarisation) — and nothing else.

It is **stateless with respect to conversation storage**. Callers pass message arrays in and get
results back; persisting anything (a compaction summary, a pruned message) is the caller's job. This
is the deliberate boundary that makes the worker reusable: a chat harness, a batch document
summariser, a RAG pipeline, or another team's bespoke agent can all call `context::assemble` without
adopting `session-manager` or any particular storage model.

The only state it keeps is operational, not conversational: short-lived compaction **leases** (so two
callers don't summarise the same logical session concurrently) under its own iii state scope.

## Standalone use

- A non-chat feature that needs "summarise these messages to fit model X" calls `context::compact`
  directly.
- A cost-sensitive caller calls `context::count-tokens` before deciding which model to use, with no
  agent loop involved.
- A different harness implementation reuses `context::assemble` as its pre-flight step and persists
  results into its own store.

## Model input

Functions that need model limits accept a `ModelInput`. Provide inline `limits` to stay fully
standalone; provide only `id`/`provider` to have the worker resolve limits via
[`router::models::get`](llm-router.md#routermodelsget) when `llm-router` is installed.

```typescript
type ModelInput = {
  id: string;
  provider?: string;
  limits?: {
    context_window: number;
    max_output_tokens: number;
    input_limit?: number;
  };
};
```

Resolution order for limits: inline `limits` -> `router::models::get(provider, id)` ->
conservative fallback (`context_window: 8192`, `max_output_tokens: 1024`). When a fallback is used,
the response sets `model_resolved: "fallback"` so callers can detect it.

## Token budget model

The usable input budget is model-adaptive, not a flat constant:

```
usable = max(0, (input_limit ?? (context_window - max_output_tokens)) - reserved - thinking_budget)
```

`reserved` defaults to `min(20000, 10% of context_window)` and is overridable per call.
`thinking_budget` is `thinking_budgets[thinking_level]` when the caller passes
`options.thinking_level` and the model declares budgets, else 0 — this is how assemble leaves room
for the reasoning tokens a thinking tier consumes. A 200k model with defaults yields ~180k usable; a
32k model yields ~12k. Compaction/pruning trigger when running tokens cross `usable`.

## Structural invariants

Whatever pruning or compaction does, the returned context must still be accepted by providers:

- **Call/result pairing.** A `function_call` and its `function_result` always land on the same side
  of any boundary: the compaction tail never starts between an assistant's call and its result
  (providers reject orphaned results), so tail selection only cuts at user/assistant turn
  boundaries.
- **Prune replaces, never removes.** Pruning rewrites a verbose output's content to a single text
  placeholder (`[output pruned: was ~N tokens]`); the block, the message, and the
  `function_call_id` linkage all survive.
- **`custom` messages are app-facing.** `context::assemble` excludes `role: "custom"` messages from
  the model-facing list (and their tokens from the count) — they have no provider wire mapping (see
  [README § Messages](README.md#messages-the-many-message-types)).

## Functions

- `context::assemble` — Build the model-ready context (system prompt + budgeted messages) from a
  history. The main "sync messages with context" entry point.
- `context::compact` — Summarise older history into a single compaction summary and return the
  preserved tail. Transient: the caller uses the result; the session keeps its full transcript.
- `context::prune` — Strip/truncate verbose function outputs without summarising. A cheaper first pass.
- `context::count-tokens` — Estimate token usage for a set of messages (+ optional invocation schema /
  system) vs a model.

## Triggers

### Trigger types emitted

None. `context-manager` is a request/response capability worker.

### Triggers bound

None by default. **Optional reactive integration:** when paired with `session-manager`, a deployment
may bind a handler to `session::message-added` to pre-warm an assembled context off the turn's hot
path (for example, to warm a cache or surface a token-usage metric). This is opt-in and lives in the
consumer, which is what keeps `context-manager` decoupled from any store — it never reaches into a
session on its own.

```typescript
iii.registerFunction("context::on_message_added", async (evt) => {
  const { messages } = await iii.trigger({
    function_id: "session::messages",
    payload: { session_id: evt.session_id },
  });
  // Measure / pre-warm; no persistence. The harness still calls context::assemble on the hot path.
  await iii.trigger({
    function_id: "context::count-tokens",
    payload: { messages: messages.map((m) => m.message), model: { id: "<model>" } },
  });
});

iii.registerTrigger({
  type: "session::message-added",
  function_id: "context::on_message_added",
  config: { /* optional: session_id, roles */ },
});
```

Future extension: a `cron`-bound `context::on_tick` for periodic long-term memory consolidation.

---

## API Reference

Shared types (`AgentMessage`, `ContentBlock`, `AgentFunction`, `Model`, `ThinkingLevel`) are defined
in [README.md § Cross-cutting contracts](README.md#cross-cutting-contracts).

### `context::assemble`

Build a model-ready context. Applies prune and/or compaction as needed to fit `usable`, in this
order: count -> (if over) prune function outputs -> (if still over) compact head -> assemble final list.

- Invocation: **sync**

Request:

```typescript
type AssembleRequest = {
  messages: AgentMessage[];        // full candidate history, oldest first
  model: ModelInput;
  system_prompt?: string;          // base system prompt to prepend/merge
  options?: {
    reserved_tokens?: number;      // override the default reserve
    tail_turns?: number;           // user+assistant pairs always kept verbatim (default 2)
    allow_compaction?: boolean;    // default true
    allow_prune?: boolean;         // default true
    protected_functions?: string[];    // function_ids whose outputs are never pruned
    thinking_level?: ThinkingLevel;    // reserve the model's thinking budget for this tier
    lease_key?: string;            // compaction mutual-exclusion key (e.g. a session id); default: hash of the message set
    previous_summary?: string;     // persisted summary from a prior compaction (see "The compaction round trip")
  };
};
```

Response:

```typescript
type AssembleResponse = {
  system_prompt: string;
  messages: AgentMessage[];        // budgeted, ready to send to llm-router
  token_count: number;             // estimated tokens of the returned context
  usable: number;                  // the budget it was fit into
  model_resolved: "inline" | "router" | "fallback";
  applied: {
    pruned: boolean;
    pruned_tokens: number;
    compacted: boolean;
    summary?: string;              // present when compacted; the caller should persist it (see below)
    tail_start_index?: number | null; // index into the request messages where the verbatim tail begins
    tokens_before?: number;
  };
};
```

Errors (thrown): `messages is required`; `could not resolve model limits` (only when neither inline
limits nor `llm-router` are available and the fallback is explicitly disabled).

Example:

```jsonc
// request
{
  "messages": [{ "role": "user", "content": [{ "type": "text", "text": "hi" }], "timestamp": 1 }],
  "model": { "id": "claude-sonnet-4", "provider": "anthropic" },
  "system_prompt": "You are a helpful assistant."
}
// response
{
  "system_prompt": "You are a helpful assistant.",
  "messages": [/* … possibly unchanged … */],
  "token_count": 24,
  "usable": 180000,
  "model_resolved": "router",
  "applied": { "pruned": false, "pruned_tokens": 0, "compacted": false }
}
```

#### The compaction round trip

`context-manager` never persists a summary — but the caller **must**, or every call past the budget
re-runs a full LLM summarisation (one extra model call per request) and summaries never converge.
The contract:

1. When `applied.compacted` is true, persist `applied.summary` and whatever your storage maps
   `applied.tail_start_index` to (the [harness](harness.md#compaction-persistence) stores both in a
   `custom` session entry with `custom_type: "compaction"`).
2. On later calls, pass only the post-compaction window as `messages` (the verbatim tail and
   everything after it) plus the stored summary as `options.previous_summary`.
3. `assemble` renders `previous_summary` into the system prompt under a `# Conversation summary`
   heading; if compaction triggers again, the summariser **updates** that summary instead of
   starting over, so it converges instead of growing.

Callers that skip step 1 still get correct output — at the cost of one summariser call per request
once over budget.

### `context::compact`

Summarise the head of a history into a single compaction summary, keeping a recent tail verbatim.
Transient and storage-agnostic: it returns the summary for the caller to use (typically
`context::assemble` applies compaction inline, so most callers never call this directly). It does not
write to any session — the durable transcript in [session-manager](session-manager.md) is untouched.

- Invocation: **sync**

Request:

```typescript
type CompactRequest = {
  messages: AgentMessage[];
  model: ModelInput;
  options?: {
    tail_turns?: number;             // default 2
    previous_summary?: string;       // anchor so summaries converge instead of growing
    preserve_recent_tokens?: number; // override adaptive tail budget
    lease_key?: string;              // mutual-exclusion key; default: hash of the message set
  };
};
```

Response (discriminated union):

```typescript
type CompactResponse =
  | { status: "ok"; summary: string; tail_start_index: number | null;
      tokens_before: number; tokens_after: number; used_prior_summary: boolean }
  | { status: "busy" }      // a compaction lease is held; caller may retry
  | { status: "empty" }     // nothing to compact
  | { status: "overflow" }; // the summariser itself overflowed
```

`tail_start_index` is an index into the request `messages` array — this worker never sees storage
entry ids. Callers that persist compaction results map the index onto their own ids (see
[harness.md § Compaction persistence](harness.md#compaction-persistence)). Tail selection respects
the [structural invariants](#structural-invariants).

The summary follows a fixed Markdown template (Goal / Constraints / Progress / Key Decisions /
Actions Taken / Next Steps / Critical Context / Relevant Files). When `previous_summary` is supplied the
summariser updates it rather than starting over.

Summariser model/provider: by default the same `model` passed in (routed through
[`router::chat`](llm-router.md#routerchat)). Compaction therefore requires `llm-router` to be present;
without it, `context::compact` returns `{ status: "overflow" }` with `error_kind: "permanent"` in the
worker log and callers should treat it as "compaction unavailable".

### `context::prune`

Replace verbose function outputs with placeholders without summarising. Walks `function_result`
content newest to oldest, freeing outputs outside a protected token window. Per the
[structural invariants](#structural-invariants), pruning rewrites content in place — it never
removes a block or message, so call/result pairing survives.

- Invocation: **sync**

Request:

```typescript
type PruneRequest = {
  messages: AgentMessage[];
  model?: ModelInput;              // only needed for token math; optional
  options?: {
    protect_recent_tokens?: number; // default 40000
    min_free_tokens?: number;       // skip if it would free less (default 20000)
    max_output_chars?: number;      // per-output truncation cap (default 2000)
    protected_functions?: string[];     // function_ids never pruned
  };
};
```

Response:

```typescript
type PruneResponse = {
  messages: AgentMessage[];   // same array with pruned outputs replaced by placeholders
  pruned_tokens: number;
  pruned_parts: number;
  scanned_parts: number;
};
```

### `context::count-tokens`

Estimate token usage for a set of messages, optionally including the invocation schema (typically the
single `agent_trigger` entry) and a system prompt.

- Invocation: **sync**

Request:

```typescript
type CountTokensRequest = {
  messages: AgentMessage[];
  system_prompt?: string;
  tools?: AgentFunction[];   // invocation schema(s) for token counting (typically [agent_trigger])
  model: ModelInput;          // tokenizer selection; falls back to a generic estimator
};
```

Response:

```typescript
type CountTokensResponse = {
  tokens: number;
  by_role?: { user: number; assistant: number; function_result: number; custom: number };
  estimator: "tokenizer" | "heuristic";
};
```

---

## State

| Scope | Key | Value | Purpose |
|---|---|---|---|
| `context_lease` | `<lease_key>` | `{ nonce, ts }` | Compaction mutual exclusion; TTL ~300s. `<lease_key>` comes from `options.lease_key` (e.g. a session id) or defaults to a hash of the message set. |

`context-manager` writes no conversation data.

## Dependencies

- `iii-state` — compaction leases only.
- `llm-router` (soft) — model limit resolution (`router::models::get`) and the summariser
  (`router::chat`). Pure token/prune calls work without it when inline `limits` are supplied.

## Agent exposure

All functions are pure transforms over caller-supplied messages — nothing secret to leak (see
[README § Security model](README.md#security-model)). The only caveat is cost: `context::assemble`
and `context::compact` can trigger a summariser LLM call. `context::count-tokens` and
`context::prune` are safe; deny the other two in cost-sensitive deployments.

## Boundaries

- Does **not** store conversations — pass messages in, persist results yourself (or use
  `session-manager`).
- Does **not** decide *when* to compact a live session on its own — that is the caller's policy (the
  harness pre-flight, or the optional reactive trigger above).
- Does **not** talk to LLM providers directly — summarisation goes through `llm-router`.
- Does **not** implement long-term/vector memory in v1; that belongs in a dedicated sibling worker.
