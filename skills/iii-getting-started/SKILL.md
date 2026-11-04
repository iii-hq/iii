---
name: iii-getting-started
description: >-
  Install the iii engine, set up your first worker, and get a working backend running. Use when a
  user wants to start a new iii project, install the SDK, or needs help with initial setup and
  configuration.
---

# Getting Started with iii

iii replaces your API framework, task queue, cron scheduler, pub/sub, state store, and observability
pipeline with a single engine and three primitives: **Worker**, **Function**, **Observer**. Workers
host functions, any worker runs a function with `call`, and `iii.observe(a, b)` means "when `a`
runs, `b` runs".

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
iii project init my-app              # barebones project
iii project init my-app -t harness   # or: the harness template (agent + console UI)
cd my-app
```

`iii project init --learn-iii` scaffolds the harness template and starts it in one step; the
installer offers the same when you answer "y" at its prompt. `iii project init --template quickstart`
scaffolds the Quickstart with a Python and a TypeScript worker.

## Step 3: Start the Project

```bash
iii compose --namespace dev --up --file worker-compose.yaml
```

The file's `engine:` section starts the engine; `containers:` starts project workers. The engine
commonly listens on `ws://localhost:49134`. Keep this foreground supervisor running.

`--namespace dev` names the daemon and becomes `III_NAMESPACE` for every worker it starts, so your
workers register and call each other in `dev`; that is why the `iii call` commands below pass
`-n dev`. Engine-owned functions (`engine::*`, `configuration::*`) stay in `default`, and SDK calls
to them resolve there without a namespace (see `iii-sdk-reference`).

## Step 4: Install the SDK

Pick your language:

```bash
# TypeScript / Node.js
pnpm add iii-sdk @iii-dev/helpers

# Python
pip install iii-sdk iii-helpers

# Rust
cargo add iii-sdk iii-helpers
```

## Step 5: Write Your First Worker

The worker registers `hello::greet` and observes `http::on-request` with a route: each
`POST /hello` request runs `hello::greet`, and its return value is the HTTP response.

### TypeScript

```typescript
import { registerWorker } from "iii-sdk";
import { Logger } from "@iii-dev/helpers/observability";

const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");

iii.registerFunction(
  "hello::greet",
  async (req: { body?: { name?: string } }) => {
    const logger = new Logger();
    const name = req.body?.name ?? "world";
    logger.info("Greeting user", { name });
    return { status_code: 200, body: { message: `Hello, ${name}!` } };
  },
  { description: "Greet a user by name" },
);

iii.observe(
  { function_id: "http::on-request", args: { method: "POST", path: "/hello" } },
  "hello::greet",
);
```

### Python

```python
from iii import register_worker, InitOptions
from iii_helpers.observability import Logger

iii = register_worker(address="ws://localhost:49134", options=InitOptions(worker_name="hello-worker"))

def greet(req):
    logger = Logger()
    body = req.get("body") or {}
    name = body.get("name", "world") if isinstance(body, dict) else "world"
    logger.info("Greeting user", {"name": name})
    return {"status_code": 200, "body": {"message": f"Hello, {name}!"}}

iii.register_function("hello::greet", greet, description="Greet a user by name")
iii.observe(
    {"function_id": "http::on-request", "args": {"method": "POST", "path": "/hello"}},
    "hello::greet",
)
```

### Rust

```rust
use iii_sdk::{register_worker, InitOptions, RegisterFunction, Source};
use iii_helpers::observability::Logger;
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(
    "hello::greet",
    RegisterFunction::new(|req: serde_json::Value| -> Result<serde_json::Value, iii_sdk::Error> {
        let logger = Logger::new();
        let name = req["body"]["name"].as_str().unwrap_or("world");
        logger.info("Greeting user", Some(json!({ "name": name })));
        Ok(json!({ "status_code": 200, "body": { "message": format!("Hello, {}!", name) } }))
    }).description("Greet a user by name"),
);

iii.observe(
    Source::function("http::on-request").args(json!({ "method": "POST", "path": "/hello" })),
    "hello::greet",
);
```

To run a function whenever another one runs, observe it the same way:
`iii.observe("hello::greet", "audit::record")` (see `iii-observe`).

## Step 6: Test It

`http::on-request` comes from the `http` worker. Add it once through the running Compose daemon,
then call your endpoint:

```bash
iii call -n dev compose::add worker=http
```

```bash
curl -X POST http://localhost:3111/hello \
  -H "Content-Type: application/json" \
  -d '{"name": "iii"}'
```

Expected response:

```json
{ "message": "Hello, iii!" }
```

## Add Existing Workers

To add a capability that already exists, browse `https://workers.iii.dev/` and add it through the
running Compose daemon:

```bash
iii call -n dev compose::add worker=state
iii call -n dev compose::add worker=queue
iii call -n dev compose::add worker=image-resize@0.1.2
```

`compose::add` resolves dependencies, writes exact versions to `worker-compose.yaml`, and restarts
the affected project. A local worker can be declared as a `path://` container or added by path.

## Install Agent Skills

Get all iii skills for your AI coding agent:

```bash
npx skills add iii-hq/iii/skills
```

Skills teach your agent the top-level iii model: workers, functions, observers, registry access,
SDKs, engine configuration, architecture patterns, and error handling. Worker-backed capabilities
live with the worker docs and registry entries.

## Adapting This Pattern

- Add more functions to the same worker: each gets its own `registerFunction`, plus an `observe`
  for each event that should run it
- Use `::` separator for function IDs to namespace them: `orders::create`, `orders::validate`
- Run a function on a schedule with `iii.observe({ function_id: 'cron::on-tick', args: {
  expression: '0 0 9 * * * *' } }, 'reports::daily')` (7-field: sec min hour day month weekday
  year; needs the `cron` worker)
- Run a function for each queue message with `iii.observe({ function_id: 'queue::on-message', args:
  { topic: 'my-queue' } }, 'jobs::process')` (needs the `queue` worker)
- Use `iii.call()` to invoke other functions from within a function
- Use `iii.observe("hello::greet", "audit::record")` to run a registered function after every
  successful call of another one (see `iii-observe`)
- Use `state::get` / `state::set` to persist data across function calls
- Use `iii call -n <daemon> compose::add worker=<name>` when the capability already exists in the
  worker registry

## Recommended Next Steps

After getting your first worker running:

1. **Register functions, call them, and create workers**: see `iii-core-primitives`
2. **Observe functions and engine events**: see `iii-observe`
3. **Choose the right SDK APIs**: see `iii-sdk-reference`
4. **Configure the engine**: see `iii-engine-config`
5. **Explore backend patterns**: see `iii-architecture-patterns`
6. **Handle failures well**: see `iii-error-handling`

## Key Resources

- [Quickstart Guide](https://iii.dev/docs/quickstart)
- [SDK Reference: Node.js](https://iii.dev/docs/reference/sdk-node)
- [SDK Reference: Python](https://iii.dev/docs/reference/sdk-python)
- [SDK Reference: Rust](https://iii.dev/docs/reference/sdk-rust)
- [Compose](https://iii.dev/docs/using-iii/compose)
- [Configuration](https://iii.dev/docs/using-iii/configuration)
- [Console](https://iii.dev/docs/using-iii/console)
- [Observers](https://iii.dev/docs/using-iii/observers)
- [Upgrading from 0.25.x](https://iii.dev/docs/upgrading/from-0-25-x)
- Every docs page is also available as Markdown by adding a `.md` suffix to its URL.

## Pattern Boundaries

- For function registration and calls, worker creation, worker registry access, built-in source
  functions, invocation modes, channels, and HTTP-invoked functions, prefer `iii-core-primitives`
- For observers, filters, engine events, event sources, and upgrading 0.25 code, prefer
  `iii-observe`
- For language-specific SDK APIs, prefer `iii-sdk-reference`
- For engine configuration, prefer `iii-engine-config`
- For worker-backed HTTP, cron, queue, pubsub, state, stream, and observability behavior, use the matching worker page on https://workers.iii.dev/
- Stay with `iii-getting-started` for installation, initial setup, and first-worker guidance

## When to Use

- Use this skill when the task is about installing iii, creating a new project, or writing a first
  worker.
- Use it when the request asks for setup help, quickstart guidance, or getting started with iii.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
