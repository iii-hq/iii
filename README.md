# iii

![iii banner image](website/og-image.png)

[![Engine License](https://img.shields.io/badge/engine-ELv2-blue.svg)](engine/LICENSE)
[![SDK License](https://img.shields.io/badge/sdk-Apache--2.0-green.svg)](sdk/LICENSE)
[![Docker](https://img.shields.io/docker/v/iiidev/iii?label=docker)](https://hub.docker.com/r/iiidev/iii)
[![npm](https://img.shields.io/npm/v/iii-sdk?label=npm)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk?label=pypi)](https://pypi.org/project/iii-sdk/)
[![Crates.io](https://img.shields.io/crates/v/iii-sdk?label=crates.io)](https://crates.io/crates/iii-sdk)

## What is iii?

Software engineering is an exercise in assembling categories of services. Each service has its own
internals, its own lifecycle, its own integration story, and its own failure modes. The cost of
every new service addition is quadratic. Every new service has potential integration points with
every other service.

iii eliminates this integration effort by reducing every new addition to zero. Installing 4 workers
or 20 workers is exactly the same. Each worker is simply able to interact with every other worker
the moment it is registered with iii. The result is an infrastructure that behaves like a single
application and composes effortlessly no matter how much it grows or changes over time.

iii supports any language, and any runtime. It makes a new engineer productive on day one because
their mental model never changes from one capability to the next. Likewise AI Agents can reliably
reason about an entire system in a single context window because there is one set of primitives to
learn and one always-accurate source of truth for what exists. As agents do more of the work of
building and operating software, small primitives compound: easier to onboard, cheaper to prompt,
faster to extend, simpler to maintain.

### Three Primitives

iii's design collapses distributed software into three concepts: Worker, Trigger, Function.
Something hosts work, something causes it, something does it.

**Workers** are processes that register with the iii engine and then register triggers and
functions. A TypeScript API service is a worker. A Python data pipeline is a worker. A Rust
microservice is a worker. Any functionality can be transformed into a worker with a few lines of
code.

**Triggers** are anything that causes a function to run. A trigger can be a direct call to a
function, an HTTP endpoint, a cron schedule, a queue subscription, a state change, a stream event,
or anything else. Triggers are declarative: the Worker defines "this function runs when this thing
happens," and iii handles routing, serialization, and delivery.

**Functions** are units of work with a stable identifier (e.g., `content::classify`,
`orders::validate`). It receives input, does work, and optionally returns output. Functions exist in
workers.

By mapping everything a service can do to these three primitives iii creates a development process
that is both effortlessly composable, and completely observable.

## Quick Start

```bash
iii project init myapp    # scaffold a project
cd myapp
iii                       # start the engine
```

Need to install `iii` first? Full walkthrough at the
[Quickstart guide](https://iii.dev/docs/quickstart).

## SDKs

| Language | Package                                            | Install                                     |
| -------- | -------------------------------------------------- | ------------------------------------------- |
| Node.js  | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `pnpm add iii-sdk` or `npm install iii-sdk` |
| Python   | [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | `pip install iii-sdk`                       |
| Rust     | [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Add to `Cargo.toml`                         |

## Agent Skills

Give your AI coding agent full context on iii:

```bash
npx skillkit add iii-hq/iii/skills
```

Skills covering every iii primitive — HTTP endpoints, queues, cron, state, streams, custom triggers,
and more. Works with Claude Code, Cursor, Gemini CLI, Codex, and
[30+ other agents](https://agentskills.io). See [skills/](skills/) for the full list.

## Console

The [iii-console](console/) is a developer and operations console for inspecting workers, functions,
triggers, queues, traces, logs, and real-time state. See the
[Console docs](https://iii.dev/docs/console) for setup and usage.

## Repository Structure

| Directory  | What it is                                              | README                                 |
| ---------- | ------------------------------------------------------- | -------------------------------------- |
| `engine/`  | iii Engine (Rust) - core runtime, modules, and protocol | [engine/README.md](engine/README.md)   |
| `sdk/`     | SDKs for Node.js, Python, and Rust                      | [sdk/README.md](sdk/README.md)         |
| `console/` | Developer console (React + Rust)                        | [console/README.md](console/README.md) |
| `skills/`  | Agent skills for AI coding agents                       | [skills/README.md](skills/README.md)   |
| `website/` | iii website                                             | [website/](website/)                   |
| `docs/`    | Documentation site (Mintlify/MDX)                       | [docs/README.md](docs/README.md)       |

See [STRUCTURE.md](STRUCTURE.md) for the full monorepo layout, dependency chain, and CI/CD details.

## Examples

See the [Quickstart guide](https://iii.dev/docs/quickstart) for step-by-step tutorials.

## Resources

- [Documentation](https://iii.dev/docs)
- [CLI & Engine](https://github.com/iii-hq/iii)
- [Console](console/)
- [Examples](https://github.com/iii-hq/iii-examples)
- [Contributing](CONTRIBUTING.md)

## License

The iii is licensed as such:

| Directory  | License                               |
| ---------- | ------------------------------------- |
| `engine/`  | [Elastic License 2.0](engine/LICENSE) |
| `sdk/`     | [Apache License 2.0](sdk/LICENSE)     |
| `console/` | [Apache License 2.0](console/LICENSE) |
| `docs/`    | [Apache License 2.0](docs/LICENSE)    |
| `website/` | [Apache License 2.0](website/LICENSE) |

The engine runtime is licensed under the Elastic License 2.0 (ELv2). All SDKs, CLI, console,
documentation, and the website are licensed under the Apache License 2.0.

See [CONTRIBUTING.md](CONTRIBUTING.md) for additional details.
