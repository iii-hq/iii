# Contributing

This is a unified monorepo containing the iii Engine, SDKs, Motia Framework, Console, documentation, and website.

## Prerequisites

- **Rust** (stable, via [rustup](https://rustup.rs/))
- **Node.js** >= 20 with **pnpm** >= 10
- **Python** >= 3.10 with **uv**

## Getting Started

```bash
# Install JS/TS dependencies
pnpm install

# Build everything (JS/TS via Turborepo)
pnpm build

# Build Rust workspace (engine + SDK + console)
cargo build --release
```

## Development Commands

### Root-level (orchestrated)

| Command | Description |
|---|---|
| `pnpm build` | Build all JS/TS packages (via Turborepo) |
| `pnpm test` | Run all JS/TS tests |
| `pnpm lint` | Lint all JS/TS packages |
| `pnpm fmt` | Format all code (Biome + cargo fmt) |
| `pnpm fmt:check` | Check formatting without changes |

### Targeted

| Command | Description |
|---|---|
| `pnpm test:sdk-node` | Test Node.js SDK only |
| `pnpm test:motia-js` | Test Motia JS only |
| `pnpm test:engine` | Test engine (Rust) only |
| `pnpm test:rust` | Test entire Rust workspace |
| `cargo test -p iii-sdk` | Test Rust SDK only |
| `pnpm dev:docs` | Start iii docs dev server from `docs/` with Mintlify |
| `pnpm dev:motia-docs` | Start Motia docs dev server |
| `pnpm dev:website` | Start website dev server |
| `pnpm dev:console` | Start console frontend dev server |

### Python

```bash
# SDK tests
cd sdk/packages/python/iii
uv sync --extra dev
uv run pytest

# Motia Python tests
cd motia/motia-py/packages/motia
uv sync --extra dev
uv run pytest
```

## How Dependencies Work

### JavaScript/TypeScript (pnpm workspaces)

The `motia` package depends on `iii-sdk` using the workspace protocol:

```json
"iii-sdk": "workspace:^"
```

This resolves to the local `sdk/packages/node/iii` during development. When publishing, pnpm replaces `workspace:^` with the actual version (e.g., `^0.3.0`).

### Rust (Cargo workspace)

The engine and console depend on `iii-sdk` as a workspace dependency:

```toml
iii-sdk = { workspace = true, features = ["otel"] }
```

This resolves to the local `sdk/packages/rust/iii` via the root `Cargo.toml` workspace config. When publishing to crates.io, Cargo substitutes the actual version.

### Python (uv editable installs)

Motia Python references the local SDK via `[tool.uv.sources]` in its `pyproject.toml`:

```toml
[tool.uv.sources]
iii-sdk = { path = "../../../../sdk/packages/python/iii", editable = true }
```

This is only used during development. Published packages use the standard PyPI version.

## CI/CD

All CI/CD runs from `.github/workflows/`.

### CI (`ci.yml`)

Runs on every push/PR to `main`. Change detection determines which jobs to run:

- **Engine changes** trigger: engine tests, all SDK tests, all Motia tests, console build
- **SDK Node changes** trigger: SDK Node tests, Motia JS tests
- **SDK Python changes** trigger: SDK Python tests, Motia Python tests
- **SDK Rust changes** trigger: SDK Rust tests, engine tests, console build
- **Motia/Console/Docs/Website changes** trigger only their own tests/builds

The engine is built from source in CI (not downloaded as a release binary), so SDK and Motia tests always validate against the current engine code.

### Release (`release.yml`)

Triggered by pushing a `release/v*` tag. Executes sequentially:

1. Run all tests
2. Build and release engine binaries (GitHub Release)
3. Publish SDKs (npm, PyPI, crates.io)
4. Publish Motia (npm, PyPI)
5. Build and release console binaries
6. Trigger package manager workflows (Homebrew, etc.)

### Creating a Release

```bash
git tag release/v0.7.0
git push origin release/v0.7.0
```

For pre-releases, add a suffix: `release/v0.7.0-alpha`, `release/v0.7.0-beta`, `release/v0.7.0-rc`.

## Licensing

This project uses a dual licensing model. The engine runtime (`engine/`) is licensed under the [Elastic License 2.0](engine/LICENSE). All other components (SDKs, CLI, console, frameworks, docs, website) are licensed under the [Apache License 2.0](sdk/LICENSE).

By submitting a contribution, you agree that your contribution will be licensed under the applicable license for the component you are contributing to.

See [NOTICE.md](NOTICE.md) for the full breakdown.

## Project Structure

See [STRUCTURE.md](STRUCTURE.md) for the full directory layout and dependency chain.
