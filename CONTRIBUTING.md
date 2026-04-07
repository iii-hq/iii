# Contributing to iii

Thank you for your interest in contributing to iii! This document explains the process
for contributing to this project and the licensing terms that apply to all contributions.

## License of Contributions

**All external contributions to this repository are made under the
[Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0).**

This applies to every part of this repository, including but not limited to:

- The **iii engine** (which is distributed under the [Elastic License 2.0](engine/LICENSE))
- Workers, SDKs, frameworks, CLI tools, and console
- Documentation, examples, tests, and configuration
- Build scripts, CI workflows, and tooling

The iii engine runtime is distributed under the Elastic License 2.0 (ELv2), but all
contributions to it are licensed under Apache 2.0. This means that by contributing, you
grant a license that permits your contributions to be incorporated into and distributed
as part of the ELv2-licensed engine, as well as any Apache 2.0-licensed components of
the project.

### What this means for you

- **You retain copyright** over your contributions. You are not assigning ownership to
  anyone.
- **You license your contributions** under Apache 2.0, which grants the project
  (and the public) a perpetual, worldwide, royalty-free, non-exclusive, irrevocable
  license to use, reproduce, modify, distribute, and sublicense your contributions.
- **Your contributions may be distributed** as part of the iii engine under the
  Elastic License 2.0 or as part of other iii components under the Apache License 2.0.
- **You can still use your own contributions** for any other purpose. The Apache 2.0
  license you grant does not restrict how you use your own work elsewhere.

### Your agreement

By submitting a pull request, issue, patch, or any other contribution to this repository,
you certify that:

1. The contribution was created in whole or in part by you and you have the right to
   submit it under the Apache License 2.0; or

2. The contribution is based upon previous work that, to the best of your knowledge,
   is covered under an appropriate open source license and you have the right under that
   license to submit that work with modifications, under the Apache License 2.0; or

3. The contribution was provided directly to you by some other person who certified (1)
   or (2) and you have not modified it.

4. You understand and agree that this project and your contribution are public, and that
   a record of the contribution (including all personal information you submit with it)
   is maintained indefinitely and may be redistributed consistent with this project's
   licenses.

If your employer has rights to intellectual property that you create (including your
contributions), you must have permission from your employer to make contributions on
their behalf, or your employer must have waived such rights for your contributions to iii.

## Getting Started

### Prerequisites

- Rust 1.80+ (edition 2024)
- Redis (only if you enable the event/cron/stream workers)

### Setting Up Your Development Environment

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/<your-username>/iii.git
   cd iii
   ```
3. Create a branch for your changes:
   ```bash
   git checkout -b my-feature
   ```
4. Make your changes and verify they build and pass lint:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```

### License Headers

Rust sources under the iii engine are checked for a license header via
[`engine/licenserc.toml`](engine/licenserc.toml), which sets `headerPath = "header.txt"`
(relative to `baseDir = "engine"`). Enforcement applies to the paths listed there (for
example `engine/src/**/*.rs` and `engine/function-macros/src/**/*.rs`), not to the whole
repository. When you add or touch files in those trees, copy the header from
[`engine/header.txt`](engine/header.txt) (or follow `engine/licenserc.toml` if the include
list changes).

## How to Contribute

### Reporting Bugs

- Search existing [issues](https://github.com/iii-hq/iii/issues) to check if the bug
  has already been reported.
- If not, open a new issue with a clear description of the problem, steps to reproduce,
  and the expected vs. actual behavior.

### Suggesting Features

- Open an issue describing the feature, the use case it serves, and how it fits within
  iii's design (Functions, Triggers, Workers).
- For larger features, please discuss in an issue before starting work so we can align
  on approach.

### Submitting Pull Requests

1. Ensure your branch is up to date with `main`.
2. Write clear, concise commit messages.
3. Include tests for new functionality where applicable.
4. Run formatting and lint checks before submitting:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```
5. Open a pull request against `main` with a description of what you changed and why.
6. Be responsive to feedback during code review.

### Code Style

- Follow Rust conventions and idioms.
- Use `cargo fmt` for formatting (the project's defaults apply).
- Use `cargo clippy` with `-D warnings` to catch common issues.
- Keep changes focused — one concern per pull request.

## Questions?

If you have questions about contributing, licensing, or anything else, feel free to open
an issue for discussion.
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

| Command          | Description                              |
| ---------------- | ---------------------------------------- |
| `pnpm build`     | Build all JS/TS packages (via Turborepo) |
| `pnpm test`      | Run all JS/TS tests                      |
| `pnpm lint`      | Lint all JS/TS packages                  |
| `pnpm fmt`       | Format all code (Biome + cargo fmt)      |
| `pnpm fmt:check` | Check formatting without changes         |

### Targeted

| Command                 | Description                                          |
| ----------------------- | ---------------------------------------------------- |
| `pnpm test:sdk-node`    | Test Node.js SDK only                                |
| `pnpm test:motia-js`    | Test Motia JS only                                   |
| `pnpm test:engine`      | Test engine (Rust) only                              |
| `pnpm test:rust`        | Test entire Rust workspace                           |
| `cargo test -p iii-sdk` | Test Rust SDK only                                   |
| `pnpm dev:docs`         | Start iii docs dev server from `docs/` with Mintlify |
| `pnpm dev:motia-docs`   | Start Motia docs dev server                          |
| `pnpm dev:website`      | Start website dev server                             |
| `pnpm dev:console`      | Start console frontend dev server                    |

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
