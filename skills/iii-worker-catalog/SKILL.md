---
name: iii-worker-catalog
description: >-
  Discover, choose, install, and learn iii workers from the worker registry.
  Use when the task needs a worker-backed capability such as harness, shell,
  sandbox, database, storage, MCP, ACP, image processing, or future workers
  published to workers.iii.dev.
---

# Worker Registry

The public worker surface is registry-first. This repo should not hardcode one static skill per worker because workers publish their own skills in `iii-hq/workers` and the registry. Use this skill to discover the right worker, install it, then load the worker-provided skill/docs before generating code.

## Flow

1. Infer the needed capability from the task: agent harness, shell tools, sandboxing, database, storage, MCP, ACP, image transforms, etc.
2. Query the registry or local directory before naming a worker.
3. Install the selected worker with `iii worker add`.
4. Sync and verify the lockfile.
5. Load the worker's own skill markdown from `iii-directory` or the registry before using worker-specific functions, trigger schemas, config, or policy details.

## Registry Discovery

Use the CLI when working locally:

```bash
iii worker add iii-directory
iii worker list
iii worker status iii-directory --no-watch
```

Use `iii-directory` from an agent or worker:

```typescript
const candidates = await iii.trigger({
  function_id: 'directory::registry::workers::list',
  payload: { search: 'harness' },
})

const info = await iii.trigger({
  function_id: 'directory::registry::workers::info',
  payload: { name: 'harness' },
})
```

Use live engine discovery after installation:

```typescript
await iii.trigger({
  function_id: 'directory::engine::workers::list',
  payload: {},
})

await iii.trigger({
  function_id: 'directory::engine::functions::list',
  payload: { search: 'session' },
})
```

## Install

Install the worker the registry returns:

```bash
iii worker add <worker>
iii worker sync
iii worker verify
iii worker start <worker>
```

Use `iii worker add`, not `cargo build`, `npm install`, or `pip install`, for normal users installing published workers. Source builds are contributor workflows.

## Worker Skills

After installing `iii-directory`, download or read worker-provided skill markdown instead of relying on this repo to know each worker's API:

```typescript
await iii.trigger({
  function_id: 'directory::skills::download',
  payload: { worker: 'harness', tag: 'latest' },
})

const index = await iii.trigger({
  function_id: 'directory::skills::index',
  payload: {},
})

const skill = await iii.trigger({
  function_id: 'directory::skills::get',
  payload: { id: 'harness/index.md' },
})
```

`directory::skills::get` accepts `id`, `<id>.md`, or `iii://<id>`. The worker skill is the source of truth for function IDs, trigger config, call payloads, installation dependencies, examples, and security notes.

## Choosing Workers

Examples of capability-to-registry searches:

- Agent runtime or durable chat: search `harness`, then read the harness worker skill.
- Host commands or filesystem tools: search `shell`, then read the shell worker skill.
- Isolation for untrusted code: search `sandbox`, then read the sandbox worker skill.
- SQL access: search `database`, then read the database worker skill.
- Object storage: search `storage`, then read the storage worker skill.
- External protocol bridges: search `mcp` or `acp`, then read that worker's skill.

Do not encode these worker APIs here. New workers can appear without a change to this repo.

## Troubleshooting

- Use `iii worker list`, `iii worker status <name> --no-watch`, and `iii worker logs <name>` for lifecycle debugging.
- Use `directory::engine::workers::info`, `directory::engine::functions::list`, and `directory::engine::triggers::list` to inspect what is actually connected.
- Use `iii worker sync --frozen` in CI and `iii worker verify` before committing worker config changes.
- If a function is missing, confirm the worker is installed, synced, started, and visible through engine discovery before changing code.

## When to Use

- Use this skill when choosing, installing, discovering, or troubleshooting any published iii worker.
- Use it when the task asks for a worker-backed capability and the exact worker/API should come from the registry or worker-provided skill.

## Boundaries

- Do not tell users to install published workers by building from source.
- Do not keep one static local skill per worker in this repo.
- Do not invent worker names, function IDs, trigger schemas, or payloads; check the registry and worker skill first.
- Do not generate removed service APIs, adapter-extension APIs, or removed stream APIs.
