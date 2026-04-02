# Motia JS/TS Monorepo

This directory hosts the JavaScript/TypeScript SDK for **Motia** — a unified backend framework that combines APIs, background jobs, queues, workflows, and AI agents into a single system powered by the **iii engine**.

## Overview

**Build production-grade backends with a single primitive.**

Motia provides:

- **APIs** — RESTful endpoints with validation and routing
- **Background Jobs** — Async processing with built-in queues
- **Durable Workflows** — Complex multi-step orchestration
- **Agentic AI** — AI agent workflows with streaming support
- **State Management** — Built-in persistent storage across steps
- **Streaming** — Real-time data updates to clients
- **Observability** — End-to-end tracing and monitoring via OpenTelemetry
- **Multi-language Support** — Write steps in TypeScript, Python, JavaScript, and more

## Repository Structure

```text
motia-js/
├── packages/
│   ├── motia/                      # Main SDK + CLI (v1.0.0-rc.22)
│   │   ├── src/                    # Source code
│   │   │   └── new/cli.ts          # CLI entry point (motia create/dev/build/typegen)
│   │   └── package.json
│   ├── stream-client/              # Core stream client library
│   ├── stream-client-browser/      # Browser stream client
│   ├── stream-client-node/         # Node.js stream client
│   └── stream-client-react/        # React hooks for streams
├── playground/                     # Dev sandbox (example Motia project)
│   ├── steps/                      # Example step implementations
│   ├── config.yaml                 # iii engine configuration
│   ├── motia.config.ts             # Motia SDK configuration
│   └── package.json
├── biome.json                      # Linter + formatter config
├── compose.yml                     # Optional Redis + RabbitMQ for production adapters
├── pnpm-workspace.yaml             # Monorepo workspace config
├── package.json                    # Root scripts and tooling
├── CONTRIBUTING.md                 # Contribution guide
└── README.md                       # SDK documentation
```

## Getting Started

### Prerequisites

- **Node.js v24+** (Volta pins v24.11.1)
- **pnpm 10+** (`pnpm@10.11.0`)
- **iii engine** — the Rust runtime ([iii.dev/docs](https://iii.dev/docs))

### Setup

1. From the repo root:

   ```bash
   cd motia-js
   pnpm install
   ```

2. Build all packages:

   ```bash
   pnpm build
   ```

3. Link the CLI for development:

   ```bash
   pnpm setup
   ```

   Reload your shell profile (`source ~/.zshrc` or `~/.bashrc`), then:

   ```bash
   pnpm link ./packages/motia --global
   motia --version
   ```

### Running the Playground

The playground is the local dev sandbox for testing Motia changes. It requires the iii engine.

```bash
pnpm dev
```

This builds all packages, then starts the playground via the iii engine. The iii engine's shell module watches for file changes and re-runs `motia dev` automatically.

Key ports:

| Port | Service |
|------|---------|
| 3111 | REST API |
| 3112 | Streams |
| 3113 | Console |
| 49134 | SDK WebSocket (iii engine) |

## Scripts

| Script | Description |
|--------|-------------|
| `pnpm build` | Build all packages (uses tsdown) |
| `pnpm dev` | Build + run playground with iii engine |
| `pnpm test` | Run unit tests (motia package) |
| `pnpm test:coverage` | Run unit tests with coverage (motia package) |
| `pnpm test:ci` | Run all tests including integration |
| `pnpm lint` | Lint with Biome |
| `pnpm lint:fix` | Auto-fix lint issues |
| `pnpm format` | Format code with Biome |
| `pnpm format:check` | Check formatting |

## How to Contribute

1. Create a new branch from `main`:

   ```bash
   git checkout -b feature/my-change
   ```

2. Make your changes:
   - SDK changes: update `packages/motia/`
   - Stream client changes: update `packages/stream-client*/`
   - Example flows: update `playground/steps/`

3. Run checks:

   ```bash
   pnpm build && pnpm lint && pnpm test
   ```

   Coverage command:

   ```bash
   pnpm test:coverage
   ```

4. Commit, push, and open a pull request.

For detailed guidelines, see [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

This project is licensed under the [Apache License 2.0](https://github.com/MotiaDev/motia/blob/main/LICENSE).
