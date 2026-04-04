![iii banner image](website/public/og-image.png)

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

Get started with iii by following the [Quickstart guide](https://iii.dev/docs/quickstart).

## SDKs

| Language | Package                                            | Install               |
| -------- | -------------------------------------------------- | --------------------- |
| Node.js  | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `npm install iii-sdk` |
| Python   | [`iii-sdk`](https://pypi.org/project/iii-sdk/)     | `pip install iii-sdk` |
| Rust     | [`iii-sdk`](https://crates.io/crates/iii-sdk)      | Add to `Cargo.toml`   |

## Console

The [iii-console](console/) is a developer and operations dashboard for inspecting functions, triggers, traces, and real-time state. See the [Console docs](https://iii.dev/docs/console) for setup and usage.

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

## Resources

- [Documentation](https://iii.dev/docs)
- [CLI & Engine](https://github.com/iii-hq/iii)
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
