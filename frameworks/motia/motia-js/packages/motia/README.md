[![Motia Banner](https://github.com/MotiaDev/motia/raw/main/assets/github-readme-banner.png)](https://motia.dev/)

[![npm version](https://img.shields.io/npm/v/motia?style=flat&logo=npm&logoColor=white&color=CB3837&labelColor=000000)](https://www.npmjs.com/package/motia)
[![license](https://img.shields.io/badge/license-Apache%202.0-blue?style=flat&logo=apache&logoColor=white&labelColor=000000)](https://github.com/MotiaDev/motia/blob/main/LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/MotiaDev/motia?style=flat&logo=github&logoColor=white&color=yellow&labelColor=000000)](https://github.com/MotiaDev/motia)
[![Twitter Follow](https://img.shields.io/badge/Follow-@motiadev-1DA1F2?style=flat&logo=twitter&logoColor=white&labelColor=000000)](https://twitter.com/motiadev)
[![Discord](https://img.shields.io/discord/1322278831184281721?style=flat&logo=discord&logoColor=white&color=5865F2&label=Discord&labelColor=000000)](https://discord.gg/motia)

**Build production-grade backends with a single primitive**

[💡 Motia Manifesto](https://www.motia.dev/manifesto) •
[🚀 Quick Start](https://www.motia.dev/docs/getting-started/quick-start) •
[📋 Defining Steps](https://www.motia.dev/docs/concepts/steps) •
[📚 Docs](https://www.motia.dev/docs)

---

## What is Motia?

Modern backends shouldn't require juggling frameworks, queues, and services. Motia unifies everything -- API endpoints, background jobs, durable workflows, AI agents, streaming, and observability -- into one runtime with a single core primitive.

**Motia** is your production-grade backend framework that combines:
- APIs, background jobs, queues, durable workflows, AI agents
- Built-in state management, streaming, and observability
- Multi-language support (JavaScript, TypeScript, Python, and more)
- Multi-runtime support (Node.js, Bun)
- All unified around one core primitive: the **Step**

Powered by the **iii engine** -- a Rust-based runtime that manages all infrastructure modules (streams, state, API, queues, cron, observability) and orchestrates SDK processes.

![Motia combines APIs, background queues, and AI agents into one system](https://github.com/MotiaDev/motia/raw/main/assets/github-readme-banner.gif)

---

## 🚀 Quickstart

Get a Motia project up and running in **under 60 seconds**:

### 1. Install the iii Engine

Motia requires the iii engine to run. Install it from [https://iii.dev](https://iii.dev).

### 2. Bootstrap a New Motia Project

```bash
npx motia@latest create   # runs the interactive terminal
```

Follow the prompts to pick a template, project name, and language.

### 3. Start the Dev Server

Inside your new project folder, launch the dev server:

```bash
iii
```

This spins up the iii engine, which manages all modules and starts your Motia SDK process with hot-reload.

### 4. Hit Your First Endpoint

Open a new terminal tab and run:

```bash
curl http://localhost:3111/default
```

You should see the JSON response:

```json
{ "message": "Hello World from Motia!" }
```

### 5. Explore the iii Console

The iii Console is your command centre for visualizing and managing your flows, traces, and infrastructure:

- **🌊 Flows** -- Visualise how your Steps connect.
- **🔌 Endpoints** -- Test APIs and stream results live.
- **👁️ Traces** -- Inspect end-to-end traces of every execution.
- **📊 Logs** -- View structured logs grouped by trace.
- **🏪 State** -- Inspect the key-value store across Steps.

---

🎉 **That's it!** You now have a fully-featured Motia project with:

- A REST API endpoint
- Visual debugger and flow inspector
- Built-in observability
- Hot-reload for instant feedback

---

## 🚧 The Problem

Backend teams juggle **fragmented runtimes** across APIs, background queues, and AI agents. This creates deployment complexity, debugging gaps, and cognitive overhead from context-switching between frameworks.

**This fragmentation demands a unified system.**

---

## 🔗 The Unified System

Motia unifies your entire backend into a **unified state**. APIs, background jobs, and AI agents become interconnected Steps with shared state and integrated observability.

| **Before**                  | **After (Motia)**                       |
| --------------------------- | --------------------------------------- |
| Multiple deployment targets | **Single unified deployment**           |
| Fragmented observability    | **End-to-end tracing**                  |
| Language dependent          | **JavaScript, TypeScript, Python, etc** |
| Context-switching overhead  | **Single intuitive model**              |
| Complex error handling      | **Automatic retries & fault tolerance** |

---

## 🔍 The Core Primitive: the Step

A Step is just a file with a `config` and a `handler`. Motia auto-discovers these files and connects them automatically. What used to be separate "step types" (API, Event, Cron) is now expressed through **triggers** -- an array of trigger definitions that describe how and when the step is activated.

A single step can have multiple triggers of different kinds, making it far more flexible than the old one-type-per-step model.

```typescript
// steps/create-user.step.ts
import type { Handlers, StepConfig } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'CreateUser',
  description: 'Create a new user',
  flows: ['user-flow'],
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/users',
      bodySchema: z.object({
        name: z.string(),
        email: z.string(),
      }),
    },
  ],
  enqueues: ['user-created'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (req, { enqueue, logger }) => {
  const { name, email } = req.body

  logger.info('Creating user', { name, email })

  await enqueue({
    topic: 'user-created',
    data: { name, email },
  })

  return { status: 200, body: { id: 'user-123' } }
}
```

---

## Trigger Types

| Type         | When it runs         | Use Case                              |
| ------------ | -------------------- | ------------------------------------- |
| **`http`**   | HTTP Request         | Expose REST endpoints                 |
| **`queue`**  | Queue subscription   | Background processing & async work    |
| **`cron`**   | Scheduled Time       | Automate recurring jobs               |
| **`state`**  | State change         | React to state mutations              |
| **`stream`** | Stream subscription  | React to real-time stream events      |

---

## How it Works

Motia's architecture is built around one powerful primitive: the **Step**. A Step is a container for your business logic -- anything from a simple database query to a complex AI agent interaction. Instead of managing separate services for APIs, background workers, and scheduled tasks, you define how your steps are triggered.

- **Need a public API?** Add an `http` trigger to your step. This defines a route and handler for HTTP requests.
- **Need a background job or queue?** Have your step `enqueue` a message. A step with a `queue` trigger subscribed to that topic will pick it up and process it asynchronously.
- **Need to run a task on a schedule?** Add a `cron` trigger to your step.
- **Need to react to state changes?** Add a `state` trigger with a condition function.
- **Need to react to stream events?** Add a `stream` trigger.

This model means you no longer need to glue together separate frameworks and tools. A single Motia application can replace a stack that might otherwise include **Nest.js** (for APIs), **Temporal** (for workflows), and **Celery/BullMQ** (for background jobs). It's all just steps and triggers.

---

## Core Concepts

The **Step** is Motia's core primitive. The following concepts are deeply integrated with Steps to help you build powerful, complex, and scalable backends:

### Steps and Triggers

Every step uses a unified `StepConfig` with a `triggers` array:

- **HTTP (`http`)** -- Build REST endpoints with schema validation and middleware.
- **Queue (`queue`)** -- Subscribe to topics for async message processing.
- **Cron (`cron`)** -- Schedule recurring jobs with 7-field cron expressions.
- **State (`state`)** -- React to state mutations with condition-based filtering.
- **Stream (`stream`)** -- React to real-time stream create/update/delete events.

### Multi-Trigger Steps

A single step can respond to multiple trigger types:

```typescript
export const config = {
  name: 'ProcessOrder',
  flows: ['orders'],
  triggers: [
    { type: 'queue', topic: 'order.created', input: orderSchema },
    { type: 'http', method: 'POST', path: '/orders/manual', bodySchema: orderSchema },
    { type: 'cron', expression: '0 * * * * * *' },
  ],
  enqueues: ['order.processed'],
} as const satisfies StepConfig
```

### Enqueue (Event-Driven Workflows)

Steps communicate by **enqueuing** messages to topics. A step declares the topics it writes to via `enqueues`, and other steps subscribe to those topics via `queue` triggers. This decouples producers from consumers and lets you compose complex workflows with simple, declarative code.

### State Management

All steps share a unified key-value state store. Every `get`, `set`, `update`, and `delete` is automatically traced. The new `update()` method supports atomic operations (`set`, `merge`, `increment`, `decrement`, `remove`) to eliminate race conditions from manual get-then-set patterns.

### Streams: Real-time Messaging

Push live updates from long-running or asynchronous workflows to clients without polling. Streams support atomic updates via `UpdateOp`, lifecycle hooks (`onJoin`, `onLeave`), and can trigger steps reactively via stream triggers.

### Structured Logging

Motia provides structured, JSON logs correlated with trace IDs and step names. Search and filter your logs without regex gymnastics.

### End-to-End Observability with Traces

Every execution generates a full trace, capturing step timelines, state operations, enqueues, stream calls, and logs. Visualise everything in the iii Console and debug faster.

---

## Configuration

Motia uses two configuration files:

### `config.yaml` (iii Engine)

All infrastructure and adapter configuration is handled by the iii engine via `config.yaml`. This includes modules for streams, state, REST API, queues, cron, observability, and SDK process management.

### `motia.config.ts` (Auth Hooks)

A simplified config file that exports only authentication hooks:

```typescript
import type { AuthenticateStream } from 'motia'

export const authenticateStream: AuthenticateStream = async (req, context) => {
  context.logger.info('Authenticating stream', { req })
  return { context: { userId: 'sergio' } }
}
```

---

## CLI Commands

### `npx motia create [options]`

Create a new Motia project in a fresh directory or the current one.

```sh
npx motia create [options]

# options
  # [project name] (optional): Project name/folder; use . or ./ to use current directory
  # -t, --template <template name>: Template to use; run npx motia templates to view available
  # -c, --cursor: Adds .cursor config for Cursor IDE
```

### `iii`

Starts the iii engine which manages all modules and the SDK process with hot-reload.

```sh
iii
```

### `motia build`

Compiles all your steps and builds a lock file based on your current project setup.

```bash
motia build
```

---

## Runtime & Module System

Motia does not enforce a specific module system or runtime. You are free to use CommonJS, ESM, Node.js, Bun, or any compatible runtime.

| Runtime | Dev Command Example     | Production Example                        |
| ------- | ----------------------- | ----------------------------------------- |
| Node.js | `npx motia dev`         | `node dist/index-production.js`           |
| Bun     | `bun run dist/index-dev.js` | `bun run --enable-source-maps dist/index-production.js` |

---

## Language Support

Write steps in your preferred language:

| Language       | Status         | Example           |
| -------------- | -------------- | ----------------- |
| **JavaScript** | Stable         | `handler.step.js` |
| **TypeScript** | Stable         | `handler.step.ts` |
| **Python**     | Stable         | `handler.step.py` |
| **Ruby**       | Beta           | `handler.step.rb` |
| **Go**         | Coming Soon    | `handler.step.go` |

Python runs as a fully independent process via the `motia-py` SDK -- no Node.js or npm required for Python-only projects.

---

## Help

For more information on a specific command, you can use the `--help` flag:

```sh
motia <command> --help
```

### Get Help

- **Questions**: Use our [Discord community](https://discord.gg/motia)
- **Bug Reports**: [GitHub Issues](https://github.com/MotiaDev/motia/issues)
- **Documentation**: [Official Docs](https://motia.dev/docs)
- **Blog**: [Motia Blog](https://blog.motia.dev/)

### Contributing

We welcome contributions! Whether it's:

- Bug fixes and improvements
- New features and step types
- Documentation and examples
- Language support additions

Check out our [Contributing Guide](https://github.com/MotiaDev/motia/blob/main/CONTRIBUTING.md) to get started.

---

**Ready to unify your backend?**

[🚀 **Get Started Now**](https://motia.dev) • [📖 **Read the Docs**](https://motia.dev/docs) • [💬 **Join Discord**](https://discord.gg/motia)

---

Built with care by the Motia team • **Star us if you find [Motia](https://github.com/MotiaDev/motia) useful!**
