# How to Contribute

Thank you for your interest in contributing to Motia! We welcome contributions from the community — whether it's fixing bugs, adding features, improving docs, or sharing examples.

## Project Overview

Motia is a unified backend framework built on the **iii engine** — a high-performance Rust runtime. The Motia SDK (JavaScript/TypeScript and Python) connects to iii via WebSocket and provides the developer-facing API for building steps, flows, and agents.

This repository is organized as a multi-language monorepo:

```text
motia/
├── motia-js/                       # JavaScript/TypeScript SDK + CLI
│   ├── packages/
│   │   ├── motia/                  # Main SDK + CLI (v1.0.0-rc.22)
│   │   ├── stream-client/          # Core stream client library
│   │   ├── stream-client-browser/  # Browser stream client
│   │   ├── stream-client-node/     # Node.js stream client
│   │   └── stream-client-react/    # React hooks for streams
│   └── playground/                 # Example Motia project (dev sandbox)
├── motia-py/                       # Python SDK
│   ├── packages/motia/             # Python SDK package
│   └── playground/                 # Python example project
├── contributors/rfc/               # RFC proposals
└── .github/                        # CI workflows, PR template, issue templates
```

## Prerequisites

- **Node.js v24+** — [Volta](https://volta.sh) pins v24.11.1 (see root `package.json`)
- **pnpm 10+** — the repo uses `pnpm@10.11.0` as its package manager
- **iii engine** — the Rust runtime that powers Motia ([install from iii.dev/docs](https://iii.dev/docs))
- **Python 3.10+** (only if working on `motia-py`) — `.python-version` pins 3.11.10
- **uv** (only if working on `motia-py`) — Python package manager used by the Python SDK

## Local Setup (JavaScript/TypeScript)

1. **Fork and clone the repository:**

   Fork [MotiaDev/motia](https://github.com/MotiaDev/motia) on GitHub, then clone your fork:

   ```bash
   git clone git@github.com:<your-username>/motia.git
   cd motia
   ```

2. **Install dependencies:**

   ```bash
   cd motia-js
   pnpm install
   ```

3. **Build all packages:**

   ```bash
   pnpm build
   ```

4. **Link the CLI globally** (for using `motia` commands during development):

   ```bash
   pnpm setup
   ```

   Then reload your shell profile before continuing:

   - **Zsh:** `source ~/.zshrc`
   - **Bash:** `source ~/.bashrc`
   - **Fish:** `source ~/.config/fish/config.fish`

   ```bash
   pnpm link ./packages/motia --global
   ```

   Verify it works:

   ```bash
   motia --version
   ```

5. **Run the playground:**

   The playground is the dev sandbox for testing changes. It requires the iii engine to be running.

   From `motia-js/playground/`:

   ```bash
   iii
   ```

   Or from `motia-js/` root:

   ```bash
   pnpm dev
   ```

   This builds all packages, then starts the playground with the iii engine.

## Local Setup (Python)

1. From the repo root:

   ```bash
   cd motia-py/packages/motia
   uv sync --extra dev
   ```

2. Run the Python playground:

   ```bash
   cd motia-py/playground
   uv sync
   iii
   ```

## How the iii Engine Works

Motia does not run your code directly — it connects to the **iii engine** via WebSocket.

- **SDK WebSocket**: `ws://localhost:49134` (where Motia SDK connects to iii)
- **REST API**: `localhost:3111` (HTTP endpoints defined by API steps)
- **Streams**: `localhost:3112` (real-time streaming)
- **Console**: `localhost:3113` (developer console UI)

The engine is configured via `config.yaml` in your project root. The playground's `config.yaml` defines modules for:

| Module | Purpose |
|--------|---------|
| `StreamModule` | Real-time streaming to clients |
| `StateModule` | Persistent key-value state across steps |
| `RestApiModule` | HTTP endpoint routing |
| `OtelModule` | OpenTelemetry observability (traces, metrics, logs) |
| `QueueModule` | Background job processing |
| `PubSubModule` | Publish/subscribe messaging |
| `CronModule` | Scheduled task execution |
| `ExecModule` | Shell commands with file watching (runs `motia dev`) |

For production, `compose.yml` provides optional Redis and RabbitMQ adapters.

## CLI Commands

The Motia CLI provides these commands:

| Command | Description |
|---------|-------------|
| `motia create` | Scaffold a new Motia project |
| `motia dev` | Build for development (used by iii shell module in watch mode) |
| `motia build` | Build for production |
| `motia typegen` | Generate TypeScript types from steps and streams |

## Linting and Formatting

This project uses **Biome** (not ESLint/Prettier) for both linting and formatting:

```bash
# From motia-js/
pnpm lint          # Check for lint issues
pnpm lint:fix      # Auto-fix lint issues
pnpm format        # Format code
pnpm format:check  # Check formatting without modifying
```

**Code style** (configured in `biome.json`):

- Single quotes
- No semicolons
- Trailing commas
- 120 character line width
- 2-space indentation

A pre-commit hook runs Biome via `lint-staged` automatically on staged JS/TS files.

**Python linting** uses **Ruff**. The pre-commit hook runs `ruff check` on staged `.py` files under `motia-py/`.

## Running Tests

```bash
# From motia-js/
pnpm test          # Unit tests only (motia package)
pnpm test:ci       # All tests including integration

# Run tests for a specific package:
pnpm --filter motia test
pnpm --filter @motiadev/stream-client test
```

Tests use **Jest** with `ts-jest`. Integration tests require a running iii engine.

**Python tests:**

```bash
cd motia-py/packages/motia
uv run pytest
```

Python tests use **pytest** with `pytest-asyncio`. Integration tests are marked with `@pytest.mark.integration`.

## Submitting Pull Requests

1. **Fork** the [Motia repository](https://github.com/MotiaDev/motia) on GitHub.
2. **Create a branch** from `main`:
   ```bash
   git checkout -b feature/my-change
   ```
3. **Make your changes** and ensure code follows the project's style (Biome will catch issues).
4. **Write tests** for new functionality where applicable.
5. **Run the checks:**
   ```bash
   cd motia-js
   pnpm build && pnpm lint && pnpm test
   ```
6. **Commit and push** to your fork.
7. **Open a pull request** against the `main` branch.

Use the [PR template](https://github.com/MotiaDev/motia/blob/main/.github/PULL_REQUEST_TEMPLATE.md) and provide a clear summary of your changes.

## RFC Process

For substantial changes (new public APIs, breaking changes, architectural decisions), submit an RFC:

1. Copy the template: `cp contributors/rfc/0000-00-00-template.md contributors/rfc/YYYY-MM-DD-my-feature.md`
2. Fill in the proposal details.
3. Submit a pull request for review.

See [`contributors/rfc/README.md`](https://github.com/MotiaDev/motia/blob/main/contributors/rfc/README.md) for the full process.

## Reporting Issues

Found a bug or have a feature request? [Open an issue](https://github.com/MotiaDev/motia/issues) with steps to reproduce and your environment details.

## Documentation

Docs are hosted at [motia.dev/docs](https://motia.dev/docs) and maintained in a separate repository. If you'd like to contribute to documentation, check the [Motia docs repo](https://github.com/MotiaDev/motia-docs).

## Community

- [Discord](https://discord.gg/motia) — chat with the team and other contributors
- [Twitter/X](https://twitter.com/motiadev) — follow for updates
- [GitHub Discussions](https://github.com/MotiaDev/motia/discussions) — longer-form conversations

We appreciate all contributions and look forward to collaborating with you!
