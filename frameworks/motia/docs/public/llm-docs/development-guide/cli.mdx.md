---
title: Command Line Interface (CLI)
description: Learn how to use the Motia CLI and the iii engine CLI to build, develop, and run your application
---

Motia projects use the **motia-cli** for scaffolding new projects, the **Motia npm package** for building Node.js projects, and the **iii CLI** for running the engine that powers everything.

---

## Motia CLI (motia-cli)

The motia-cli scaffolds new Motia projects with support for Node.js, Python, or mixed language templates. Python developers don't need npm; Node.js developers don't need Python.

### Install motia-cli

```bash
curl -fsSL https://raw.githubusercontent.com/MotiaDev/motia-cli/main/install.sh | sh
```

Or via Homebrew:

```bash
brew tap MotiaDev/tap
brew install motia-cli
```

### `motia-cli create [name]`

Scaffold a new Motia project with sensible defaults including `iii-config.yaml`, example Steps, and the appropriate dependency files for your chosen language.

```bash
motia-cli create my-project
```

Interactive prompts will ask for:

- **Project folder name** — or pass it as an argument
- **Language** — Node.js (TypeScript), Python, or Mixed (both)
- **iii installation** — confirms you have the iii engine installed

The CLI downloads templates from the [motia-iii-example](https://github.com/MotiaDev/motia-iii-example) repository and installs dependencies automatically (`npm install` for Node.js, `uv sync` for Python).

---

## iii CLI

The iii engine is the runtime that manages all infrastructure modules (queues, state, streams, cron, HTTP) and your Motia application process.

### Install iii

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

### `iii`

Start the iii engine. By default it looks for `config.yaml` in the current directory.

```bash
iii
```

### `iii -c <config-file>`

Start the engine with a specific configuration file.

```bash
iii -c config.yaml
iii -c config-production.yaml
```

### `iii -v`

Print the iii engine version.

```bash
iii -v
```

<Callout type="info">
During development, your `package.json` should have `"dev": "iii"` so that `npm run dev` starts the engine, which reads `config.yaml` and boots all configured modules including your Motia application via the ExecModule.
</Callout>

---

## iii Console

The iii console provides a visual interface for building and debugging flows. Install and run it separately:

```bash
curl -fsSL https://install.iii.dev/console/main/install.sh | sh
iii-console --enable-flow
```

Then open [http://localhost:3113/](http://localhost:3113/) to see flow diagrams, real-time logs, state inspection, and stream monitoring.

![iii Console Dashboard](/console/dashboard.png)

---

## Motia Node.js Tools (npx motia)

For Node.js and mixed projects, the `motia` npm package provides build and development tools. Install it as a project dependency when you scaffold with motia-cli.

### `motia build`

Build your project, generating an optimized production bundle in `dist/`.

```bash
npx motia build
```

This produces:
- `dist/index-production.js`
- `dist/index-production.js.map`

Your `config.yaml` should reference the production bundle in the ExecModule (place this under the top-level `modules:` list):

```yaml title="config.yaml (production ExecModule)"
modules:
  # ... other modules (StreamModule, StateModule, etc.) ...

  - class: modules::shell::ExecModule
    config:
      exec:
        - bun run --enable-source-maps dist/index-production.js
```

### `motia dev`

Start the Motia SDK in development mode. This is typically run automatically by the iii engine's ExecModule, not manually.

```bash
npx motia dev
```

Your `config.yaml` ExecModule for development (place this under the top-level `modules:` list). The `watch` array defines file patterns that trigger a rebuild, and the `exec` commands run sequentially — `npx motia dev` compiles the Steps, then `node dist/index-dev.js` starts the built output:

```yaml title="config.yaml (development ExecModule)"
modules:
  # ... other modules (StreamModule, StateModule, etc.) ...

  - class: modules::shell::ExecModule
    config:
      watch:
        - steps/**/*.ts
      exec:
        - npx motia dev
        - node dist/index-dev.js
```

### `motia rules pull`

Update AI development guides (Cursor rules, AGENTS.md) to the latest version.

```bash
npx motia rules pull
npx motia rules pull --force  # Overwrite existing
```

---

## Typical Development Workflow

```bash
motia-cli create my-project   # Scaffold a new project (choose Node.js, Python, or Mixed)
cd my-project
iii -c iii-config.yaml        # Starts the iii engine and your Motia Steps
```

The `iii` engine reads `iii-config.yaml`, starts all modules, and uses the ExecModule to build and run your Motia Steps automatically. File changes trigger hot-reload.

---

## What's Next?

<Cards>
  <Card href="/docs/concepts/overview" title="Core Concepts">
    Learn about Steps, triggers, and the event-driven architecture
  </Card>
  <Card href="/docs/examples" title="Examples">
    Explore real-world patterns and use cases
  </Card>
</Cards>
