![iii - One Engine, Three Primitives](engine/assets/banner.jpg)

[![Engine License](https://img.shields.io/badge/engine-ELv2-blue.svg)](engine/LICENSE)
[![SDK License](https://img.shields.io/badge/sdk-Apache--2.0-green.svg)](sdk/LICENSE)
[![Docker](https://img.shields.io/docker/v/iiidev/iii?label=docker)](https://hub.docker.com/r/iiidev/iii)
[![npm](https://img.shields.io/npm/v/iii-sdk?label=npm)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk?label=pypi)](https://pypi.org/project/iii-sdk/)
[![Crates.io](https://img.shields.io/crates/v/iii-sdk?label=crates.io)](https://crates.io/crates/iii-sdk)

## What is iii

You start building a backend and immediately need six different tools: an API framework, a task queue, a cron scheduler, pub/sub, a state store, and an observability pipeline. Each has its own config, its own deployment, its own failure modes. A simple "process this, then notify that" workflow touches three services before you write any business logic.

iii replaces all of that with a single engine and three primitives: **Function**, **Trigger**, and **Worker**.

A Function is anything that does work. A Trigger is what causes it to run - an HTTP request, a cron schedule, a queue message, a state change. A Worker connects your functions to the engine. You write the function, declare what triggers it, connect a worker, and the engine handles routing, retries, and observability.

One config file. One process. Everything discoverable. Think of it the way React gave frontend a single model for UI - iii gives your backend a single model for execution.

## Three Concepts

| Concept       | What it does |
| ------------- | ------------ |
| **Function**  | A unit of work. It receives input and optionally returns output. It can exist anywhere: locally, in the cloud, on serverless, or as a third-party HTTP endpoint. |
| **Trigger**   | What causes a Function to run - explicitly from code, or automatically from an event source. Examples: HTTP route, cron schedule, queue topic, state change, stream event. |
| **Worker**    | The runtime that connects your functions and triggers to the engine. Workers register and deregister themselves without configuration. Once connected, their functions are available across the entire backend. |

## Quick Start

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
iii-cli start --use-default-config
```

Your engine is running at `ws://localhost:49134` with HTTP API at `http://localhost:3111`.

For a project-backed setup, create `config.yaml` in your working directory or pass `iii-cli start --config /path/to/config.yaml`.

### Connect a worker

```bash
npm install iii-sdk
```

```javascript
import { registerWorker, Logger } from 'iii-sdk';

const iii = registerWorker('ws://localhost:49134');
const logger = new Logger();

iii.registerFunction({ id: 'math.add' }, async (input) => {
  return { sum: input.a + input.b };
});

iii.registerTrigger({
  type: 'http',
  function_id: 'math.add',
  config: { api_path: 'add', http_method: 'POST' },
});

const result = await iii.trigger({ function_id: 'math.add', payload: { a: 1, b: 2 } });
logger.info('result', result); // { sum: 3 }
```

Your function is now live at `http://localhost:3111/add`.

<details>
<summary>Python</summary>

```bash
pip install iii-sdk
```

```python
from iii import register_worker, Logger

iii = register_worker("ws://localhost:49134")
logger = Logger()

def add(data):
    return {"sum": data["a"] + data["b"]}

iii.register_function({"id": "math.add"}, add)

iii.register_trigger({
    "type": "http",
    "function_id": "math.add",
    "config": {"api_path": "add", "http_method": "POST"}
})

result = iii.trigger({"function_id": "math.add", "payload": {"a": 1, "b": 2}})
logger.info("result", result)  # {"sum": 3}
```

</details>

<details>
<summary>Rust</summary>

```rust
use iii_sdk::{register_worker, InitOptions, TriggerRequest, RegisterFunctionMessage, RegisterTriggerInput, Logger};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default())?;
    let logger = Logger::new();

    iii.register_function(RegisterFunctionMessage { id: "math.add".into(), description: None, request_format: None, response_format: None, metadata: None, invocation: None }, |input| async move {
        let a = input.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
        let b = input.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(json!({ "sum": a + b }))
    });

    iii.register_trigger(RegisterTriggerInput { trigger_type: "http".into(), function_id: "math.add".into(), config: json!({
        "api_path": "add",
        "http_method": "POST"
    }) })?;

    let result = iii.trigger(TriggerRequest::new("math.add", json!({ "a": 1, "b": 2 }))).await?;
    logger.info("result", &result); // {"sum":3}

    Ok(())
}
```

</details>

## SDKs

| Language | Package                                            | Install               |
| -------- | -------------------------------------------------- | --------------------- |
| Node.js  | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `npm install iii-sdk` |
| Python   | [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | `pip install iii-sdk` |
| Rust     | [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Add to `Cargo.toml`   |

## Console

The [iii-console](console/) is a developer and operations dashboard for inspecting functions, triggers, traces, and real-time state.

```bash
iii-cli console
```

![iii console dashboard](https://raw.githubusercontent.com/iii-hq/docs/main/public/docs/console/dashboard-dark.png)

## Repository Structure

| Directory      | What it is                                              | README                                  |
| -------------- | ------------------------------------------------------- | --------------------------------------- |
| `engine/`      | iii Engine (Rust) - core runtime, modules, and protocol | [engine/README.md](engine/README.md)    |
| `sdk/`         | SDKs for Node.js, Python, and Rust                      | [sdk/README.md](sdk/README.md)          |
| `console/`     | Developer dashboard (React + Rust)                      | [console/README.md](console/README.md)  |
| `frameworks/`  | Higher-level frameworks built on the SDK                 | [frameworks/motia/](frameworks/motia/)  |
| `website/`     | iii website                                              | [website/](website/)                    |
| `docs/`        | Documentation site (Mintlify/MDX)                       | [docs/README.md](docs/README.md)        |


See [STRUCTURE.md](STRUCTURE.md) for the full monorepo layout, dependency chain, and CI/CD details.

## Examples

See the [Quickstart guide](https://iii.dev/docs/quickstart) for step-by-step tutorials.

## Docs Development

The iii documentation lives in [`docs/`](docs/), which is a Mintlify docs site driven by [`docs/docs.json`](docs/docs.json).

```bash
pnpm dev:docs
```

This runs `npx mint dev` inside `docs/` and serves the docs locally, typically at `http://localhost:3000`.

## Resources

- [Documentation](https://iii.dev/docs)
- [CLI](https://github.com/iii-hq/iii-cli)
- [Console](console/)
- [Examples](https://github.com/iii-hq/iii-examples)
- [Contributing](CONTRIBUTING.md)

## License

The iii project uses a dual licensing model:

| Directory        | License              |
| ---------------- | -------------------- |
| `engine/`        | [Elastic License 2.0](engine/LICENSE) |
| `sdk/`           | [Apache License 2.0](sdk/LICENSE) |
| `cli/`           | [Apache License 2.0](cli/LICENSE) |
| `console/`       | [Apache License 2.0](console/LICENSE) |
| `frameworks/`    | [Apache License 2.0](frameworks/motia/LICENSE) |
| `docs/`          | [Apache License 2.0](docs/LICENSE) |
| `website/`       | [Apache License 2.0](website/LICENSE) |

The engine runtime is licensed under the Elastic License 2.0 (ELv2). All SDKs, frameworks, CLI, console, documentation, and the website are licensed under the Apache License 2.0.

See [NOTICE.md](NOTICE.md) for details.
