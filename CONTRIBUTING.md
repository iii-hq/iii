# Contributing to III Engine

Thank you for your interest in contributing to iii Engine! This document provides guidelines and information to help you contribute effectively.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Development Workflow](#development-workflow)
- [Code Style](#code-style)
- [Testing](#testing)
- [Pull Request Process](#pull-request-process)
- [License Headers](#license-headers)
- [Project Structure](#project-structure)
- [SDK Contributions](#sdk-contributions)
- [Reporting Issues](#reporting-issues)

## Code of Conduct

We are committed to providing a welcoming and inclusive experience for everyone. Please be respectful and constructive in all interactions.

## Getting Started

### Prerequisites

- **Rust 1.80+** - Install via [rustup](https://rustup.rs/)
- **Redis** (optional) - Required for event bus, state, and cron modules
- **RabbitMQ** (optional) - Alternative event bus adapter
- **pre-commit** - For Git hooks (`pip install pre-commit`)

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/iii-engine.git
   cd iii-engine
   ```
3. Add upstream remote:
   ```bash
   git remote add upstream https://github.com/MotiaDev/iii-engine.git
   ```

## Development Setup

### Install Dependencies

```bash
# Install Rust toolchain components
rustup component add rustfmt clippy

# Install pre-commit hooks
pre-commit install

# Install Hawkeye for license header management (cross-platform)
cargo install hawkeye
```

### Build the Project

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Build for specific target
cargo build --release --target x86_64-unknown-linux-gnu
```

### Run the Engine

```bash
# With default config
cargo run -- --config config.yaml

# With debug logging
RUST_LOG=debug cargo run -- --config config.yaml

# Watch mode (auto-rebuild on changes)
make watch-debug
```

## Development Workflow

### Creating a Feature Branch

Always create a feature branch for your work:

```bash
git checkout main
git pull upstream main
git checkout -b feature/your-feature-name
```

### Making Changes

1. Write your code following the [code style guidelines](#code-style)
2. Add or update tests as needed
3. Ensure all tests pass locally
4. Update documentation if applicable

### Keeping Your Branch Updated

```bash
git fetch upstream
git rebase upstream/main
```

## Code Style

### Formatting and Linting

We enforce strict code quality standards:

```bash
# Format code
cargo fmt --all

# Check formatting (CI will fail if this fails)
cargo fmt --all -- --check

# Run linter (all warnings are errors)
cargo clippy --all-targets --all-features -- -D warnings
```

### Pre-commit Hooks

Pre-commit hooks run automatically on each commit:

- **cargo fmt** - Code formatting
- **cargo check** - Compilation check
- **cargo clippy** - Linting with `-D warnings`
- **YAML validation** - Config file validation
- **Trailing whitespace** - Cleanup

If a hook fails, fix the issues and try committing again.

### Style Guidelines

- Use meaningful variable and function names
- Keep functions focused and small
- Document public APIs with rustdoc comments
- Prefer `Result` over `panic!` for error handling
- Use `tracing` for logging, not `println!`

## Testing

### Running Tests

```bash
# Run all tests
cargo test --all-features

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture
```

### Writing Tests

- Place unit tests in the same file using `#[cfg(test)]` modules
- Use `mockall` for mocking dependencies
- Test both success and error paths
- Name tests descriptively: `test_function_behavior_when_condition`

Example:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscribe_adds_to_subscribers() {
        let adapter = BuiltInPubSubLite::new(None);
        // ... test implementation
    }
}
```

## Pull Request Process

### Before Submitting

1. **Rebase on main** - Ensure your branch is up to date
2. **Run all checks locally**:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-features
   hawkeye check
   ```
3. **Write a clear commit message** - Describe what and why

### PR Guidelines

- **One feature per PR** - Keep PRs focused and reviewable
- **Link related issues** - Use "Fixes #123" or "Relates to #123"
- **Describe your changes** - Explain the what, why, and how
- **Add screenshots** - For UI changes, include before/after
- **Be responsive** - Address review feedback promptly

### CI Checks

All PRs must pass:

- **Formatting** - `cargo fmt` check
- **Linting** - `cargo clippy` with no warnings
- **Tests** - All tests must pass
- **License Headers** - Hawkeye validation
- **Multi-platform builds** - macOS, Windows, Linux

## License Headers

The III Engine core is licensed under **Elastic License 2.0 (ELv2)**. All Rust source files in `src/` and `function-macros/src/` must include the license header.

### Check for Missing Headers

```bash
# Using Hawkeye CLI
hawkeye check

# Using Docker (no install required)
docker run -it --rm -v $(pwd):/github/workspace ghcr.io/korandoru/hawkeye check
```

### Add Missing Headers

```bash
# Using Hawkeye CLI
hawkeye format

# Using Docker
docker run -it --rm -v $(pwd):/github/workspace ghcr.io/korandoru/hawkeye format
```

### Header Template

```rust
// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.
```

## Project Structure

```
iii-engine/
├── src/                    # Core engine source (ELv2)
│   ├── main.rs            # CLI entry point
│   ├── lib.rs             # Library exports
│   ├── engine/            # Worker management, routing
│   ├── modules/           # Core modules (event, cron, state, etc.)
│   ├── builtins/          # Built-in functions (kv, queue, pubsub)
│   ├── workers/           # Worker trait definitions
│   └── invocation/        # Invocation lifecycle
├── function-macros/       # Proc macro library (ELv2)
├── packages/              # Multi-language SDKs (Apache 2.0)
│   ├── node/             # Node.js/TypeScript SDK
│   └── rust/             # Rust SDK
├── examples/              # Example implementations
├── .github/workflows/     # CI/CD pipelines
├── config.yaml           # Example configuration
└── Cargo.toml            # Rust package manifest
```

## SDK Contributions

SDKs are licensed under **Apache 2.0** and do not require ELv2 headers.

### Node.js SDK

```bash
cd packages/node
pnpm install
pnpm build
pnpm test
```

### Rust SDK

```bash
cd packages/rust/iii
cargo build
cargo test
```

## Reporting Issues

### Bug Reports

Include:
- **III Engine version** - `iii --version`
- **Operating system** - e.g., Ubuntu 22.04, macOS 14, Windows 11
- **Steps to reproduce** - Minimal example to trigger the bug
- **Expected behavior** - What should happen
- **Actual behavior** - What actually happens
- **Logs** - Relevant error messages or stack traces

### Feature Requests

Include:
- **Problem statement** - What problem does this solve?
- **Proposed solution** - How should it work?
- **Alternatives considered** - Other approaches you've thought of
- **Use cases** - Who benefits and how?

## Questions?

- Open a [GitHub Discussion](https://github.com/MotiaDev/iii-engine/discussions)
- Reach out at support@motia.dev

Thank you for contributing to iii Engine!
