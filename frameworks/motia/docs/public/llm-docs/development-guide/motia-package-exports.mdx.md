---
title: motia Package Exports
description: Reference for what the motia npm package exports in v1.x.
---

This page documents the public exports of the `motia` npm package.

## Install

```bash
npm install motia
```

## Package entrypoints

`motia` currently publishes these entrypoints:

| Entrypoint | Purpose |
|---|---|
| `motia` | Main runtime/framework API used by Steps |
| `motia/build` | Build-time utilities |

## Main exports (`motia`)

These are exported from the root package:

### Runtime primitives

- `step`
- `multiTriggerStep`
- `http`, `queue`, `cron`, `state`, `stream`, `api`

`api` is a legacy alias kept for backward compatibility. Prefer `http` for all new code.

### Runtime services

- `enqueue`
- `logger`
- `stateManager`
- `Stream`

### Initialization and internals

> **Internal use only.** These exports are implementation details of the iii engine integration and may change without notice. Do not rely on them in application code.

- `initIII`, `getInstance`
- `setupStepEndpoint`
- `generateStepId`
- `Motia`

### Types and helpers

- All exports from `./types`
- All exports from `./types-stream`
- All exports from `./guards`
- `StreamAuthInput`, `StreamAuthResult`, `StreamContext` (from `iii-sdk/stream`)
- `Logger`, `ChannelReader`, `ChannelWriter` (from `iii-sdk`)

## `motia/build` exports

The package defines a `motia/build` subpath export intended for build workflows and tooling integration. Common use:

```typescript
import { buildConfig } from 'motia/build'
```

This entrypoint is primarily consumed by CLI tooling (`motia build`) and bundler integrations. See the [CLI reference](/docs/development-guide/cli) for usage in project build scripts.

## Important note about React stream hooks

React stream hooks are provided by a separate package:

```bash
npm install @motiadev/stream-client-react
```

Use:

- `MotiaStreamProvider`
- `useMotiaStream`
- `useStreamGroup`
- `useStreamItem`

See [React Stream Client](/docs/development-guide/react-stream-client).
