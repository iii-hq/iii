# Motia Framework Removal Summary

This document records the Motia framework patterns that were removed from iii-prefixed skills when they were rebuilt to match the actual iii documentation at https://iii.dev/docs.

## Why Motia Was Removed

The iii documentation describes only the `iii-sdk` interfaces (TypeScript/Node.js, Python, Rust). The skills previously included parallel "Motia Framework" examples alongside the SDK examples. Since Motia patterns are not documented at iii.dev/docs, they were removed to eliminate confusion and ensure skills match the actual interfaces.

## Motia Concepts That Were Removed

| Concept | Description | iii-sdk Equivalent |
|---------|-------------|-------------------|
| `StepConfig` | Declarative step configuration object | `registerFunction` + `registerTrigger` |
| `Handlers<typeof config>` | Typed handler from step config | Async handler function passed to `registerFunction` |
| `step(config, handler)` | Step factory combining config + handler | `registerFunction` + `registerTrigger` separately |
| `queue('topic', { input })` | Queue trigger shorthand | `registerTrigger({ type: 'queue', config: { topic } })` |
| `http('METHOD', '/path')` | HTTP trigger shorthand | `registerTrigger({ type: 'http', config: { api_path, http_method } })` |
| `cron('expression')` | Cron trigger shorthand | `registerTrigger({ type: 'cron', config: { expression } })` |
| `ctx.match({ http, queue, cron })` | Dispatch by trigger type | Separate functions per trigger type |
| `FlowContext` | Context with logger, state, enqueue | `getContext()` for logger; `iii.trigger('state::*', ...)` for state |
| `ctx.enqueue({ topic, data })` | Enqueue work | `iii.trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` |
| `ctx.state.set/get/list/delete` | State access on context | `iii.trigger('state::set/get/list/delete', { scope, key, value })` |
| `ctx.getData()` | Get trigger input data | Input is the first argument to the handler function |
| `flows: ['name']` | Flow grouping | No equivalent (organizational only) |
| `enqueues: ['topic']` | Declared output topics | No equivalent (implicit in code) |
| `StreamConfig` | Stream step configuration | `registerFunction` + stream API |
| `useStream` (React) | React hook for streams | `StreamClient.subscribe()` or WebSocket directly |
| `ApiRequest` / `ApiResponse` (Motia) | Motia request/response types | SDK `ApiRequest` / `ApiResponse` with `status_code` |
| `Stream` (Motia Python) | Python stream helper | `iii.trigger('stream::set/get', ...)` |
| `bodySchema` / `responseSchema` | Zod schema validation | Manual validation in handler |
| `TriggerCondition` | Typed condition function | `registerFunction` returning boolean + `condition_function_id` in trigger config |

## Per-Skill Removal Details

### iii-rest-api
- Removed: Motia Framework Pattern (TypeScript) section with `StepConfig`, `Handlers`, `bodySchema`, `responseSchema`
- Removed: Motia Framework Pattern (Python) section with `http()`, `ApiRequest`, `ApiResponse`, `FlowContext`

### iii-cron-tasks
- Removed: Motia Framework Pattern (TypeScript) with `StepConfig`, `Handlers`, `state.list`, `enqueue`
- Removed: Motia Framework Pattern (Python) with `cron()`, `FlowContext`, `ctx.enqueue`

### iii-background-jobs
- Removed: Motia Framework Pattern (TypeScript) with `step()`, `queue()`, `stepConfig`, `ctx.getData()`, `ctx.state.set`, `ctx.enqueue`
- Removed: Motia Framework Pattern (Python) with `queue()`, `FlowContext`, `ctx.enqueue`

### iii-state-management
- Removed: Motia Framework: State in Steps with `step()`, `queue()`, `ctx.state.set/list`, `ctx.enqueue`
- Removed: Motia Framework: Streams (Python) with `Stream` class, `greetings_stream.set()`

### iii-realtime-streaming
- Removed: Motia Framework: Stream Steps with `StreamConfig`, stream handler
- Removed: Motia Framework: SSE Step with `StepConfig`, `Handlers`
- Removed: Browser Client (React) with `useStream` hook from `motia/stream-client-react`

### iii-workflows
- Removed: Motia Framework Pattern (TypeScript) — 3 multi-step workflow sections with `StepConfig`, `Handlers`, `step()`, `queue()`, `ctx.getData()`, `ctx.state.set/get`, `ctx.enqueue`
- Removed: Motia Framework Pattern (Python) with `queue()`, `FlowContext`, `ctx.enqueue`, `ctx.state.set`
- Removed: Parallel Fan-Out Pattern with `step()`, `ctx.enqueue`

### iii-multi-trigger
- Removed: Entire Motia-based `ctx.match()` multi-trigger pattern (TypeScript and Python)
- Removed: `TriggerCondition` typed condition functions
- Removed: `isHighValue`, `is_business_hours` condition examples

### iii-ai-agents
- Removed: RAG Pipeline (Motia) — 3-step pattern with `StepConfig`, `Handlers`, `step()`, `queue()`, `ctx.state.set`, `ctx.enqueue`

### iii-python-sdk
- No Motia sections (was already SDK-focused)

### iii-rust-sdk
- No Motia sections (was already SDK-focused)

### iii-deployment
- No Motia sections (was already config/infrastructure-focused)

### iii-observability
- No Motia sections (was already SDK-focused)

### iii-custom-triggers
- No Motia sections (was already SDK-focused)

### iii-pubsub
- No Motia sections (was already SDK-focused)

### iii-channels
- No Motia sections (was already SDK-focused)

### iii-testing
- Removed: Motia step mock patterns with `vi.mock('motia')`, mock `FlowContext`, mock `ctx.state`, mock `ctx.enqueue`
