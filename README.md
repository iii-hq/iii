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

## Why iii

Every service comes with its own runtime, API, and failure modes. Getting services to work together means endless integration and instrumentation work: error handling, gateways, sync, tracing, and maintenance that never stops.

It's estimated that up to 70% of development time is dedicated to integrations. 40% is the lowest estimate we could find.

iii removes that work with one primitive, the **Function**. Services, agents, and tools connect to one engine as workers, and everything they can do is a Function that any other worker can call or observe, in any language and on any machine. Workers can then be defined in a Compose file and run on any infrastructure. An entire service stack can be brought up, down, upgraded, or swapped out with ease.

iii is like having an entire app platform at your fingertips without the vendor lock-in. The result is a system that is composable, observable, discoverable, extensible, and reactive. See [Built for C.O.D.E.R.](#built-for-coder).

## One primitive: the Function

iii organizes software around one primitive, the **Function**, and two operators:

- `worker.call(a)` runs function `a` now.
- `worker.observe(a, b)` makes function `b` run when function `a` runs.

Workers host functions. Queues, databases, HTTP, streaming, sandboxes, observability, agents, and business logic are all functions. An HTTP request is a call of `http::on-request`. A database write is a call of `database::on-row-changed`. A queue message is a call of `queue::on-message`. To react to any of them, you use the same `observe`.

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
const observer = worker.observe(
  { function_id: 'orders::create', filters: { 'input.total': { $gte: 500 } } },
  'alerts::big-order',
)
await observer.ready()

// Run orders::create now and wait for its result
const order = await worker.call({ function_id: 'orders::create', payload: { item: 'widget', total: 750 } })
console.log(order)
```

The same API is in every SDK. See [sdk/README.md](sdk/README.md).

## Built for C.O.D.E.R.

| | Property | What you get |
| --- | --- | --- |
| **C** | Composability | Functions in different languages and locations work together seamlessly. |
| **O** | Observability | One trace follows every execution step across languages and machines. Export over OTLP. |
| **D** | Discoverability | The engine holds a live registry of every Function. It's always up to date so developers and agents have a clear picture. |
| **E** | Extensibility | Workers can be added and removed at any time with a single command. |
| **R** | Reactivity | Anything can be an observer or observed. Reactivity and events do not need to be pre-planned. |

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

In one terminal, create and start a project:

```bash
iii project init my-app && cd my-app
iii compose --up   # start the engine and the project's workers; keep it running
```

In a second terminal, add a worker from the registry:

```bash
cd my-app
iii call compose::add worker=http
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
- iii Workers, docs, and SDKs: [Apache License 2.0](sdk/LICENSE)

All contributions are made under Apache 2.0. See [CONTRIBUTING.md](CONTRIBUTING.md#license-of-contributions) for full contribution and licensing details.
