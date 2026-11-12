<div align="center">

# iii

**更多应用，更少基础设施。**

将任何函数、智能体、API、模型、浏览器、基础设施和机器连接成一个实时、可组合的系统。

[![GitHub stars](https://img.shields.io/github/stars/iii-hq/iii?style=flat&logo=github)](https://github.com/iii-hq/iii/stargazers)
[![CI](https://img.shields.io/github/actions/workflow/status/iii-hq/iii/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white)](https://github.com/iii-hq/iii/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/iii-hq/iii?label=release)](https://github.com/iii-hq/iii/releases)
[![Contributors](https://img.shields.io/github/contributors/iii-hq/iii)](https://github.com/iii-hq/iii/graphs/contributors)
[![License](https://img.shields.io/badge/license-Apache--2.0%20%2B%20ELv2-blue)](#许可证)

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

[官网](https://iii.dev) · [文档](https://iii.dev/docs) · [快速入门](https://iii.dev/docs/quickstart) · [Worker 注册表](https://iii.dev/registry) · [博客](https://iii.dev/blog) · [宣言](https://iii.dev/manifesto)

[English](README.md) | 简体中文

</div>

## 为什么选择 iii

每个服务都有自己的运行时、API 和故障方式。连接它们需要网关、重试、同步、链路追踪，以及永远需要维护的胶水代码。每个新服务都要与技术栈中已有的每个服务做集成：四个服务有六条可能的集成边，二十个服务有 190 条。

iii 用一个原语 **Function** 消除这些工作。服务、智能体和工具作为 worker 连接到同一个引擎，它们能做的一切都是函数，任何其他 worker 都可以调用（call）或观察（observe）它，不论使用什么语言、运行在哪台机器上。HTTP 路由、队列、数据库变更、智能体和业务逻辑都以同样的方式工作，因此添加第两百个 worker 与添加第二个是同一个操作。

结果是一个可组合、可观测、可发现、可扩展且具备响应能力的系统。参见[为 C.O.D.E.R. 而构建](#为-coder-而构建)。

## 一个原语：Function

iii 围绕一个原语 **Function** 和两个操作来组织软件：

- `worker.call(a)` 立即运行函数 `a`。
- `worker.observe(a, b)` 使函数 `a` 运行时，函数 `b` 也运行。

worker 托管函数。队列、数据库、HTTP、流、沙箱、可观测性、智能体和业务逻辑都是函数。一个 HTTP 请求是对 `http::on-request` 的一次调用。一次数据库写入是对 `database::on-row-changed` 的一次调用。一条队列消息是对 `queue::on-message` 的一次调用。要响应其中任何一个，都使用同一个 `observe`。

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

每个 SDK 都提供相同的 API。参见 [sdk/README.md](sdk/README.md)。

## 为 C.O.D.E.R. 而构建

| | 特性 | 你得到什么 |
| --- | --- | --- |
| **C** | 可组合性（Composability） | 不同语言的函数通过同一个引擎协同工作。 |
| **O** | 可观测性（Observability） | 一条 trace 跨语言、跨机器跟踪一个请求。日志附着在写入它的 span 上。可通过 OTLP 导出。 |
| **D** | 可发现性（Discoverability） | 引擎维护所有 worker 和函数的实时注册表。先搜索运行中的系统，再搜索 worker 注册表。 |
| **E** | 可扩展性（Extensibility） | `compose::add` 添加一个 worker 并解析其依赖。其他所有 worker 可以立即调用它。 |
| **R** | 响应性（Reactivity） | observer 使系统具备响应能力。为一个事件添加新的响应时，事件源不需要改变。 |

## 安装

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

安装程序会询问是否设置你的第一个项目。按 Enter，然后打开 http://127.0.0.1:3113。

在 CI、Docker 构建和脚本中，可以不经提示直接安装：

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- --non-interactive
```

[安装指南](https://iii.dev/docs/install)列出了所有安装选项。

## 快速入门

在第一个终端中创建并启动项目：

```bash
iii project init my-app && cd my-app
iii compose --up   # start the engine and the project's workers; keep it running
```

在第二个终端中从注册表添加一个 worker：

```bash
cd my-app
iii call compose::add worker=http
```

引擎监听 `ws://localhost:49134`。控制台运行在 http://127.0.0.1:3113。下一步请阅读[快速入门](https://iii.dev/docs/quickstart)。

## SDK

| 语言 | 包 | 安装 |
| --- | --- | --- |
| TypeScript（Node.js） | [`iii-sdk`](https://www.npmjs.com/package/iii-sdk) | `pnpm add iii-sdk` |
| TypeScript（浏览器） | [`iii-browser-sdk`](https://www.npmjs.com/package/iii-browser-sdk) | `pnpm add iii-browser-sdk` |
| Python | [`iii-sdk`](https://pypi.org/project/iii-sdk/) | `pip install iii-sdk` |
| Rust | [`iii-sdk`](https://crates.io/crates/iii-sdk) | `cargo add iii-sdk` |
| Go | [`iii`](https://pkg.go.dev/github.com/iii-hq/iii/sdk/packages/go/iii) | `go get github.com/iii-hq/iii/sdk/packages/go/iii` |

## 仓库结构

| 路径 | 内容 |
| --- | --- |
| [`engine/`](engine/README.md) | Rust 引擎和 `iii` CLI |
| [`sdk/`](sdk/README.md) | TypeScript、Python、Rust 和 Go 的 SDK |
| [`docs/`](docs/README.md) | [iii.dev/docs](https://iii.dev/docs) 的文档源码（Mintlify） |
| [`website/`](website/README.md) | [iii.dev](https://iii.dev) 的源码 |
| [`skills/`](skills/) | 面向编码智能体的 agent skills |
| [`tech-specs/`](tech-specs/) | 技术规范 |

## 贡献

欢迎贡献。请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 了解环境设置、命令和贡献许可条款。

```bash
pnpm install && pnpm build   # JS/TS packages
cargo build --release        # engine and Rust SDK
pnpm test && cargo test      # tests
```

请在 [GitHub issues](https://github.com/iii-hq/iii/issues) 中报告 bug 和提出功能请求。

## 社区

- [Discord](https://discord.gg/iiidev)：提问与讨论
- [X](https://x.com/iiidevs) 和 [LinkedIn](https://www.linkedin.com/company/iii-dev)：新闻
- [博客](https://iii.dev/blog)：架构文章和完整示例
- [路线图](https://iii.dev/roadmap)：后续计划

## 许可证

- `engine/`（引擎和 CLI）：[Elastic License 2.0](engine/LICENSE)
- 其他所有内容，包括 SDK：[Apache License 2.0](sdk/LICENSE)

所有贡献均以 Apache 2.0 许可提交。参见 [CONTRIBUTING.md](CONTRIBUTING.md#license-of-contributions)。
