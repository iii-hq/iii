---
title: Welcome to Motia
description: "Build production-grade backends with a single primitive. APIs, background jobs, workflows, AI agents, streaming, state management, and observability — unified in one framework."
---

**Motia** is a unified backend framework where everything is built around one core primitive: **the Step**. Instead of juggling separate frameworks for APIs, queues, cron jobs, AI agents, and real-time streaming, you write Steps — simple files with a `config` and a `handler` — and Motia connects them automatically.

```typescript
import { enqueue, logger, type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'CreateUser',
  triggers: [{ type: 'http', method: 'POST', path: '/users' }],
  enqueues: ['user.created'],
  flows: ['onboarding'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Creating user', { email: request.body.email })
  await enqueue({ topic: 'user.created', data: request.body })
  return { status: 201, body: { success: true } }
}
```

Drop this file in your project and Motia auto-discovers it. No registration, no boilerplate wiring.

---

## What You Get

Motia gives you a complete backend toolkit from a single primitive:

| Capability | How It Works |
|---|---|
| **HTTP APIs** | Steps with `http` triggers become REST endpoints with validation and routing |
| **Background Jobs** | Steps with `queue` triggers process messages asynchronously with retries |
| **Scheduled Tasks** | Steps with `cron` triggers run on a schedule |
| **Reactive Workflows** | Steps with `state` or `stream` triggers react to data changes automatically |
| **AI Agents** | Build agentic workflows with streaming support and tool calling |
| **State Management** | Built-in key-value storage shared across all Steps, with atomic updates |
| **Real-Time Streaming** | Push live updates to connected clients via WebSocket streams |
| **Observability** | Structured logging, distributed tracing, and metrics out of the box |
| **Multi-Language** | Write Steps in TypeScript, Python, or JavaScript — they all work together |

---

## Powered by iii

Motia runs on the [iii engine](https://iii.dev), which manages all infrastructure concerns — queues, pub/sub, state storage, stream servers, cron scheduling, and observability — through a single `config.yaml` file. You focus on business logic in your Steps; iii handles the rest.



---

### Prerequisites

- **iii** — Install the runtime: `curl -fsSL https://install.iii.dev/iii/main/install.sh | sh`
- **Node.js 18+** — Required for TypeScript/JavaScript Steps
- **Python 3** — Optional, for Python Steps

---

## Get Started

<Cards>
  <Card title="Quick Start" href="/docs/getting-started/quick-start">
    Clone the example project and run your first flow in under 2 minutes.
  </Card>
  <Card title="Core Concepts" href="/docs/concepts/overview">
    Understand Steps, triggers, and the event-driven architecture.
  </Card>
  <Card title="Why Motia?" href="/docs/why-motia">
    Learn why Motia exists and what problems it solves.
  </Card>
  <Card title="Examples" href="https://github.com/MotiaDev/motia-examples">
    Explore real-world examples covering APIs, AI agents, workflows, and more.
  </Card>
</Cards>
