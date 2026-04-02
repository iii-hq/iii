---
title: Multi-Language Development
description: Build Motia applications with TypeScript, Python, and JavaScript running as independent runtimes
---

Motia supports writing Steps in multiple languages within the same project. Each language runtime runs as an independent process managed by the iii engine — there are no cross-language dependencies. Python developers do not need Node.js, and Node.js developers do not need Python.

## How It Works

Each language has its own Motia SDK and its own process. The iii engine coordinates between them through shared infrastructure (queues, state, streams). A TypeScript Step can enqueue a message that a Python Step processes, and vice versa.

```text
┌──────────────────────────────────┐
│            iii Engine            │
│                                  │
│  ┌───────────┐  ┌───────────┐    │
│  │ ExecModule│  │ ExecModule│    │
│  │ (Node.js) │  │ (Python)  │    │
│  └─────┬─────┘  └─────┬─────┘    │
│        ▼               ▼         │
│  ┌───────────┐  ┌───────────┐    │
│  │ Motia SDK │  │ Motia SDK │    │
│  │ (TS/JS)   │  │ (Python)  │    │
│  └───────────┘  └───────────┘    │
│                                  │
│  Shared: Queues, State, Streams  │
└──────────────────────────────────┘
```

## Supported Languages

| Language | File Pattern | SDK Package | Runtime |
|---|---|---|---|
| TypeScript | `.step.ts` | `motia` (npm) | Node.js or Bun |
| JavaScript | `.step.js` | `motia` (npm) | Node.js or Bun |
| Python | `_step.py` | `motia` (pip) | Python 3 |

## config.yaml Setup

Configure separate ExecModule entries for each runtime:

```yaml
modules:
  # Node.js / TypeScript runtime
  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.ts
        - steps/**/*.js
        - motia.config.ts
      exec:
        - npx motia dev

  # Python runtime
  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.py
      exec:
        - uv run motia dev --dir steps
```

Each ExecModule watches its own file patterns and manages its own SDK process independently.

## Example: Cross-Language Flow

A TypeScript HTTP endpoint triggers a Python ML processing step:

```typescript title="steps/submit-review.step.ts"
import { type Handlers, type StepConfig, enqueue } from 'motia'
import { z } from 'zod'

export const config = {
  name: 'SubmitReview',
  description: 'Accepts a product review for analysis',
  triggers: [
    { type: 'http', method: 'POST', path: '/reviews' },
  ],
  enqueues: ['review.submitted'],
  flows: ['review-pipeline'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  await enqueue({ topic: 'review.submitted', data: { text: request.body.text } })
  return { status: 202, body: { status: 'processing' } }
}
```

```python title="steps/analyze_review_step.py"
from motia import state_manager, enqueue

config = {
    "name": "AnalyzeReview",
    "description": "Runs sentiment analysis on the review",
    "triggers": [
        {"type": "queue", "topic": "review.submitted"}
    ],
    "enqueues": ["review.analyzed"],
    "flows": ["review-pipeline"]
}

async def handler(input, ctx):
    text = input.get("text", "")
    sentiment = analyze_sentiment(text)

    await state_manager.set("reviews", ctx.trace_id, {
        "text": text,
        "sentiment": sentiment,
        "analyzed": True
    })

    await enqueue({
        "topic": "review.analyzed",
        "data": {"traceId": ctx.trace_id, "sentiment": sentiment}
    })

def analyze_sentiment(text):
    # Your ML model here
    return "positive"
```

Both Steps share the same flow (`review-pipeline`) and communicate through the queue — no direct inter-process communication needed.

## Mixed Template Example

The [motia-iii-example](https://github.com/MotiaDev/motia-iii-example) repository includes a **mixed** template that demonstrates Node.js and Python working together in a single project. Create it with:

```bash
motia-cli create my-project
# Select "Mixed (Node.js + Python, requires both)" when prompted
```

The mixed template structure:

| Directory | Responsibility |
|-----------|----------------|
| `nodejs/src/` | HTTP API endpoints (`create-ticket`, `list-tickets`) |
| `python/steps/` | Queue and cron triggers (`triage`, `notify`, `sla-monitor`, `escalate`) |

The `iii-config.yaml` uses two ExecModules — one for each runtime:

```yaml
modules:
  # ... shared infrastructure (StreamModule, StateModule, RestApiModule, etc.) ...

  - class: modules::shell::ExecModule
    config:
      watch: ["nodejs/src/**/*.ts"]
      exec:
        - sh -c "cd nodejs && npx motia dev"
        - sh -c "cd nodejs && node dist/index-dev.js"

  - class: modules::shell::ExecModule
    config:
      watch: ["python/steps/**/*.py"]
      exec:
        - sh -c "cd python && uv run motia run --dir steps"
```

When a user creates a ticket via `POST /tickets`, the Node.js Step stores it in state and enqueues to `ticket::created`. The Python Step consumes that topic, triages the ticket, and enqueues to `ticket::triaged`. Both runtimes share the same state, queues, and API — proving true multi-language orchestration without cross-dependencies.

## Runtime Flexibility

### Node.js vs Bun

Motia supports both Node.js and Bun for TypeScript/JavaScript Steps. Configure your preferred runtime in the ExecModule:

```yaml
# Using Node.js (default)
- class: modules::shell::ExecModule
  config:
    exec:
      - npx motia dev

# Using Bun (requires building first with `bun run build`)
- class: modules::shell::ExecModule
  config:
    exec:
      - bun run dist/index-dev.js
```

### Module System

You can use either CommonJS or ESM. For ESM (recommended for Bun compatibility):

```json title="package.json"
{
  "type": "module"
}
```

```json title="tsconfig.json"
{
  "compilerOptions": {
    "module": "ESNext",
    "moduleResolution": "bundler",
    "moduleDetection": "force"
  }
}
```

CommonJS works without any changes. Motia does not force a module system migration.
