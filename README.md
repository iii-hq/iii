<div align="center">

# iii

**More application. Less infrastructure.**

Connect any functions, agents, APIs, models, browsers, infrastructure, and machines into one live, composable system.

[![GitHub stars](https://img.shields.io/github/stars/iii-hq/iii?style=flat&logo=github)](https://github.com/iii-hq/iii/stargazers)
[![CI](https://img.shields.io/github/actions/workflow/status/iii-hq/iii/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white)](https://github.com/iii-hq/iii/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/iii-hq/iii?label=release)](https://github.com/iii-hq/iii/releases)
[![Contributors](https://img.shields.io/github/contributors/iii-hq/iii)](https://github.com/iii-hq/iii/graphs/contributors)
[![License](https://img.shields.io/badge/license-Apache--2.0%20%2B%20ELv2-blue)](#license)

[![npm](https://img.shields.io/npm/v/iii-sdk?label=npm&logo=npm)](https://www.npmjs.com/package/iii-sdk)
[![npm downloads](https://img.shields.io/npm/dw/iii-sdk?label=npm%20downloads)](https://www.npmjs.com/package/iii-sdk)
[![PyPI](https://img.shields.io/pypi/v/iii-sdk?label=PyPI&logo=pypi&logoColor=white)](https://pypi.org/project/iii-sdk/)
[![PyPI downloads](https://img.shields.io/pypi/dm/iii-sdk?label=PyPI%20downloads)](https://pypistats.org/packages/iii-sdk)
[![crates.io](https://img.shields.io/crates/v/iii-sdk?logo=rust)](https://crates.io/crates/iii-sdk)
[![docs.rs](https://img.shields.io/docsrs/iii-sdk?logo=docsdotrs)](https://docs.rs/iii-sdk)
[![Go Reference](https://pkg.go.dev/badge/github.com/iii-hq/iii/sdk/packages/go/iii.svg)](https://pkg.go.dev/github.com/iii-hq/iii/sdk/packages/go/iii)
[![Docker pulls](https://img.shields.io/docker/pulls/iiidev/iii?logo=docker&logoColor=white)](https://hub.docker.com/r/iiidev/iii)

[![Discord](https://img.shields.io/discord/1322278831184281721?label=Discord&logo=discord&logoColor=white)](https://discord.gg/iiidev)
[![X](https://img.shields.io/twitter/follow/iiidevs?style=flat&logo=x)](https://x.com/iiidevs)

[Website](https://iii.dev) · [Docs](https://iii.dev/docs) · [Quickstart](https://iii.dev/docs/quickstart) · [Worker registry](https://iii.dev/registry) · [Blog](https://iii.dev/blog) · [Manifesto](https://iii.dev/manifesto)

English | [简体中文](README_zh.md)

</div>

## What is iii

iii (pronounced "three eye") is an engine and one open protocol (JSON over WebSocket). Services, agents, and tools connect to the engine as **workers**. Each worker registers **functions**. Every call between them goes through the engine.

Each new service usually adds integration work with every service already in the stack. Four services have six possible integration edges. Twenty services have 190. With iii, adding two workers or two hundred is the same operation.

## One primitive: the Function

iii organizes software around one primitive, the **Function**, and two operators:

- `worker.call(a)` runs function `a` now.
- `worker.observe(a, b)` makes function `b` run when function `a` runs.

Workers host functions. Queues, cron, HTTP, streaming, sandboxes, observability, agents, and business logic are all functions. An HTTP request is a call of `http::on-request`. A cron tick is a call of `cron::on-tick`. A queue message is a call of `queue::on-message`. To react to any of them, you use the same `observe`.

```typescript
import { registerWorker } from 'iii-sdk'

const worker = registerWorker('ws://localhost:49134')

worker.registerFunction('orders::create', async (order: { item: string; total: number }) => {
  return { id: crypto.randomUUID(), ...order }
})

worker.registerFunction('alerts::big-order', async ({ input }: { input: { total: number } }) => {
  console.log(`Big order: ${input.total}`)
})

// When orders::create runs with a total of 500 or more, alerts::big-order runs
worker.observe(
  { function_id: 'orders::create', filters: { 'input.total': { $gte: 500 } } },
  'alerts::big-order',
)

// Run orders::create now and wait for its result
const order = await worker.call({ function_id: 'orders::create', payload: { item: 'widget', total: 750 } })
console.log(order)
```

The same API is in every SDK. See [sdk/README.md](sdk/README.md).

## Built for C.O.D.E.R.

| | Property | What you get |
| --- | --- | --- |
| **C** | Composability | Functions in different languages work together through the same engine. |
| **O** | Observability | One trace follows a request across languages and machines. Logs stay attached to the span that wrote them. Export over OTLP. |
| **D** | Discoverability | The engine holds a live registry of every worker and function. Search the running system, then the worker registry. |
| **E** | Extensibility | `compose::add` adds a worker and resolves its dependencies. Every other worker can call it immediately. |
| **R** | Reactivity | Observers make the system reactive. Add a new reaction to an event, and the event source does not change. |

## Install

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

The installer asks to set up your first project. Press Enter, then open http://127.0.0.1:3113.

For CI, Docker builds, and scripts, install without the prompt:

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- --non-interactive
```

The [install guide](https://iii.dev/docs/install) has all installer options.

## Quickstart

```bash
iii project init my-app && cd my-app   # create a project
iii compose --up                       # start the engine and the project's workers
iii call compose::add worker=http      # add a worker from the registry
```

The engine listens on `ws://localhost:49134`. The console serves at http://127.0.0.1:3113. Continue with the [Quickstart](https://iii.dev/docs/quickstart).

## SDKs

| Language | Package | Install |
| --- | --- | --- |
| TypeScript (Node.js) | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `pnpm add iii-sdk` |
| TypeScript (browser) | [`iii-browser-sdk`](https://www.npmjs.com/package/iii-browser-sdk) | `pnpm add iii-browser-sdk` |
| Python | [`iii-sdk`](https://pypi.org/project/iii-sdk/) | `pip install iii-sdk` |
| Rust | [`iii-sdk`](https://crates.io/crates/iii-sdk) | `cargo add iii-sdk` |
| Go | [`iii`](https://pkg.go.dev/github.com/iii-hq/iii/sdk/packages/go/iii) | `go get github.com/iii-hq/iii/sdk/packages/go/iii` |

## Repository structure

| Path | Contents |
| --- | --- |
| [`engine/`](engine/README.md) | Rust engine and `iii` CLI |
| [`sdk/`](sdk/README.md) | SDKs for TypeScript, Python, Rust, and Go |
| [`docs/`](docs/README.md) | Documentation source for [iii.dev/docs](https://iii.dev/docs) (Mintlify) |
| [`website/`](website/README.md) | Source for [iii.dev](https://iii.dev) |
| [`skills/`](skills/) | Agent skills for coding agents |
| [`tech-specs/`](tech-specs/) | Technical specifications |

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, commands, and the contribution license terms.

```bash
pnpm install && pnpm build   # JS/TS packages
cargo build --release        # engine and Rust SDK
pnpm test && cargo test      # tests
```

Report bugs and request features in [GitHub issues](https://github.com/iii-hq/iii/issues).

## Community

- [Discord](https://discord.gg/iiidev): questions and discussion
- [X](https://x.com/iiidevs) and [LinkedIn](https://www.linkedin.com/company/iii-dev): news
- [Blog](https://iii.dev/blog): architecture posts and worked examples
- [Roadmap](https://iii.dev/roadmap): what comes next

## License

- `engine/` (engine and CLI): [Elastic License 2.0](engine/LICENSE)
- Everything else, including the SDKs: [Apache License 2.0](sdk/LICENSE)

All contributions are made under Apache 2.0. See [CONTRIBUTING.md](CONTRIBUTING.md#license-of-contributions).
