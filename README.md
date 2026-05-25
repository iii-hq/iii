# iii

![iii: point-to-point integrations vs zero-integration via shared runtime](.github/assets/zero-integration.png)

[![Engine License](https://img.shields.io/badge/engine-ELv2-blue.svg)](engine/LICENSE)
[![SDK License](https://img.shields.io/badge/sdk-Apache--2.0-green.svg)](sdk/LICENSE)
[![Docker](https://img.shields.io/docker/v/iiidev/iii?label=docker)](https://hub.docker.com/r/iiidev/iii)
[![npm](https://img.shields.io/npm/v/iii-sdk?label=npm)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk?label=pypi)](https://pypi.org/project/iii-sdk/)
[![Crates.io](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fcrates.io%2Fapi%2Fv1%2Fcrates%2Fiii-sdk&query=%24.crate.max_stable_version&label=crates.io&prefix=v&color=orange)](https://crates.io/crates/iii-sdk)

## What is iii?

iii is the easiest way to compose, extend, and observe every service in your stack in real time.

Every backend starts as a project before the first line of business logic. Queues, cron, HTTP,
state, observability, agents, and sandboxes each usually bring their own integration story. iii
collapses that into one live system surface.

```bash
iii worker add queue
iii worker add agent
iii worker add sandbox
iii worker add <anything>
```

Each worker joins the live catalog. Every other worker is notified and can call it immediately.
Browse available workers at [workers.iii.dev](https://workers.iii.dev/).

That is the agent story too: when a task needs a capability the system does not have, an agent can
add a worker, discover its functions, call them, and trace what happened. Same interface a developer
uses.

### Three Primitives

Worker _ Function _ Trigger is the entire mental model.

**Workers** are processes that register with the iii engine and then register triggers and
functions. A TypeScript API service is a worker. A Python data pipeline is a worker. A Rust
microservice is a worker. Any functionality can be transformed into a worker with a few lines of
code. Workers can also create other workers at runtime, so agents and applications can extend the
system while it is running.

**Triggers** are anything that causes a function to run. A trigger can be a direct call to a
function, an HTTP endpoint, a cron schedule, a queue subscription, a state change, a stream event,
or anything else. Triggers are declarative: the Worker defines "this function runs when this thing
happens," and iii handles routing, serialization, and delivery.

**Functions** are units of work with a stable identifier (e.g., `content::classify`,
`orders::validate`). It receives input, does work, and optionally returns output. Functions exist in
workers.

By mapping everything a service can do to these three primitives iii creates a development process
that is both effortlessly composable, and completely observable.

## What Changes

Before iii:

- New observability tool: uncountable integrations
- New agent harness: separate retry config, separate traces, separate timeouts
- New queue: vendor evaluation, procurement, and weeks of integration

After iii:

- `iii worker add observability`
- `iii worker add queue`
- Done. It is in the system, traceable, and callable.

Platform teams publish workers. Application teams register functions and declare triggers. Agents
use the same catalog and the same function calls.

Extending iii is `iii worker add`. Composing iii is calling functions. Observing iii is opening the
trace.

## Quick Start

```bash
iii project init myapp    # scaffold a project
cd myapp
iii                       # start the engine
```

Need to install `iii` first? Full walkthrough at the
[Quickstart guide](https://iii.dev/docs/quickstart).

## Add Workers

Install new capabilities into a project with `iii worker add`:

[![Adding a worker with iii worker add](.github/assets/workers-add.gif)](https://workers.iii.dev/)

## SDKs

| Language | Package                                            | Install                                     |
| -------- | -------------------------------------------------- | ------------------------------------------- |
| Node.js  | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `pnpm add iii-sdk` or `npm install iii-sdk` |
| Python   | [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | `pip install iii-sdk`                       |
| Rust     | [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Add to `Cargo.toml`                         |

## Agent Skills

Install iii's agent-readable reference material:

```bash
npx skills add iii-hq/iii/skills
```

Skills cover every iii primitive: HTTP endpoints, queues, cron, state, streams, custom triggers, and
more. See [skills/](skills/) for the full list.

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
| `skills/`  | Agent-readable reference material                       | [skills/README.md](skills/README.md)   |
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
