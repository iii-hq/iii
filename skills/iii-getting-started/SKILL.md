---
name: iii-getting-started
description: >-
  Install the iii engine, set up your first worker, and get a working backend running. Use when a
  user wants to start a new iii project, install the SDK, or needs help with initial setup and
  configuration.
---

# Getting Started with iii

iii replaces your API framework, task queue, cron scheduler, pub/sub, state store, and observability
pipeline with a single engine and three primitives: **Function**, **Trigger**, **Worker**.

## Step 1: Install the Engine

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

Verify it installed:

```bash
iii --version
```

## Step 2: Create a Project

```bash
iii create
```

Follow the interactive prompts to select a template and language. The default quickstart template
includes TypeScript, Python, and Rust workers.

Then change into the project directory you chose at the prompt:

```bash
cd <your-project>
```

## Step 3: Start the Engine

```bash
iii --config iii-config.yaml
```

The engine starts and listens for worker connections on `ws://localhost:49134`. The REST API is
available at `http://localhost:3111`. The console is available at `http://localhost:3113`.

## Step 4: Install the SDK

Pick your language:

```bash
# TypeScript / Node.js
npm install iii-sdk

# Python
pip install iii-sdk

# Rust
cargo add iii-sdk
```

## Step 5: Write Your First Worker

### TypeScript

```typescript
import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");

iii.registerFunction(
  "hello::greet",
  async (input) => {
    const logger = new Logger();
    const name = input?.name ?? "world";
    logger.info("Greeting user", { name });
    return { message: `Hello, ${name}!` };
  },
  { description: "Greet a user by name" },
);

iii.registerTrigger({
  type: "http",
  function_id: "hello::greet",
  config: { api_path: "/hello", http_method: "POST" },
});
```

### Python

```python
from iii import register_worker, InitOptions, Logger

iii = register_worker(address="ws://localhost:49134", options=InitOptions(worker_name="hello-worker"))

def greet(data):
    logger = Logger()
    name = data.get("name", "world") if isinstance(data, dict) else "world"
    logger.info("Greeting user", {"name": name})
    return {"message": f"Hello, {name}!"}

iii.register_function("hello::greet", greet, description="Greet a user by name")
iii.register_trigger({"type": "http", "function_id": "hello::greet", "config": {"api_path": "/hello", "http_method": "POST"}})
```

### Rust

```rust
use iii_sdk::{register_worker, InitOptions, Logger, RegisterFunction, RegisterTriggerInput};
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(
    RegisterFunction::new("hello::greet", |input: serde_json::Value| -> Result<serde_json::Value, String> {
        let logger = Logger::new();
        let name = input["name"].as_str().unwrap_or("world");
        logger.info("Greeting user", Some(json!({ "name": name })));
        Ok(json!({ "message": format!("Hello, {}!", name) }))
    }).description("Greet a user by name"),
);

iii.register_trigger(RegisterTriggerInput {
    trigger_type: "http".into(),
    function_id: "hello::greet".into(),
    config: json!({ "api_path": "/hello", "http_method": "POST" }),
    metadata: None,
})?;
```

## Step 6: Test It

```bash
curl -X POST http://localhost:3111/hello \
  -H "Content-Type: application/json" \
  -d '{"name": "iii"}'
```

Expected response:

```json
{ "message": "Hello, iii!" }
```

## Install Agent Skills

Get all iii skills for your AI coding agent:

```bash
npx skills add iii-hq/iii/skills
```

Skills teach your agent the top-level iii model: functions, triggers, workers, SDKs, engine
configuration, channels, and architecture patterns. Worker-backed capabilities live with the engine
worker docs.

## Adapting This Pattern

- Add more functions to the same worker — each gets its own `registerFunction` + `registerTrigger`
  calls
- Use `::` separator for function IDs to namespace them: `orders::create`, `orders::validate`
- Add cron triggers with `{ type: 'cron', config: { expression: '0 0 9 * * * *' } }` (7-field: sec
  min hour day month weekday year)
- Add queue triggers with `{ type: 'durable:subscriber', config: { topic: 'my-queue' } }`
- Use `iii.trigger()` to invoke other functions from within a function
- Use `state::get` / `state::set` to persist data across function calls

## Recommended Next Steps

After getting your first worker running:

1. **Register more functions and triggers** — See `iii-functions-and-triggers`
2. **Choose invocation modes** — See `iii-trigger-actions`
3. **Validate trigger payloads** — See `iii-trigger-schemas`
4. **Configure the engine** — See `iii-engine-config`
5. **Move binary data between workers** — See `iii-channels`
6. **Explore architecture patterns** — See `iii-agentic-backend`, `iii-reactive-backend`,
   `iii-workflow-orchestration`

## Key Resources

- [Quickstart Guide](https://iii.dev/docs/quickstart)
- [SDK Reference — Node.js](https://iii.dev/docs/api-reference/sdk-node)
- [SDK Reference — Python](https://iii.dev/docs/api-reference/sdk-python)
- [SDK Reference — Rust](https://iii.dev/docs/api-reference/sdk-rust)
- [Engine Configuration](https://iii.dev/docs/configuration)
- [Console](https://iii.dev/docs/console)

## Pattern Boundaries

- For function and trigger registration patterns, prefer `iii-functions-and-triggers`
- For trigger payload schemas, prefer `iii-trigger-schemas`
- For invocation mode choices, prefer `iii-trigger-actions`
- For engine configuration, prefer `iii-engine-config`
- For worker-backed HTTP, cron, queue, pubsub, state, stream, and observability behavior, use the matching worker docs under `engine/src/workers/**/skills`
- Stay with `iii-getting-started` for installation, initial setup, and first-worker guidance

## When to Use

- Use this skill when the task is about installing iii, creating a new project, or writing a first
  worker.
- Triggers when the request asks for setup help, quickstart guidance, or getting started with iii.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
