## iii

Internal notes for contributors and AI agents working in this monorepo. Use `README.md` as the public source of truth for project overview. See `CONTRIBUTING.md` for build commands and development workflow.

### Architecture

iii is a backend unification engine built on three primitives: **Function** (unit of work), **Trigger** (what causes it to run), and **Worker** (runtime connecting functions to the engine). One config file, one process, everything discoverable.

### Repository Structure

| Directory | Language | What it is |
| --- | --- | --- |
| `engine/` | Rust | Core runtime — modules (HTTP, cron, queue, state, stream, pubsub, otel), protocol, CLI |
| `sdk/packages/node/iii/` | TypeScript | Node.js SDK (`iii-sdk` on npm) |
| `sdk/packages/python/iii/` | Python | Python SDK (`iii-sdk` on PyPI) |
| `sdk/packages/rust/iii/` | Rust | Rust SDK (`iii-sdk` on crates.io) |
| `console/` | React + Rust | Developer dashboard for inspecting functions, triggers, traces, state |
| `frameworks/motia/` | TypeScript/Python | Higher-level framework built on the SDK |
| `skills/` | Markdown + JS/Py/Rs | 24 agent skills for AI coding agents (auto-discovered by SkillKit) |
| `docs/` | MDX (Mintlify) | Documentation site at iii.dev/docs |
| `website/` | TypeScript | iii.dev website |
| `scripts/` | Shell | Build and CI scripts |

### Important Files

- `Cargo.toml` — Rust workspace root; members: engine, engine/function-macros, sdk/packages/rust/iii, sdk/packages/rust/iii-example, console/packages/console-rust
- `pnpm-workspace.yaml` — pnpm workspace; includes sdk/node, frameworks/motia, console, docs
- `turbo.json` — Turborepo config for JS/TS build orchestration
- `biome.json` — Linter/formatter config for JS/TS
- `engine/config.yaml` — Default engine configuration (modules, ports, adapters)
- `engine/src/main.rs` — Engine entrypoint
- `engine/src/lib.rs` — Engine library root
- `engine/src/modules/` — Engine modules (http, cron, queue, state, stream, pubsub, otel, channels)
- `sdk/packages/node/iii/src/index.ts` — Node.js SDK entrypoint
- `sdk/packages/python/iii/iii_sdk/` — Python SDK source
- `sdk/packages/rust/iii/src/lib.rs` — Rust SDK entrypoint
- `skills/references/iii-config.yaml` — Full annotated engine config reference for skills

### Build Commands

```bash
# JS/TS (all packages)
pnpm install && pnpm build && pnpm test

# Rust (engine + SDK + console)
cargo build --release && cargo test

# Python SDK
cd sdk/packages/python/iii && uv sync --extra dev && uv run pytest

# Engine only
cargo run --release              # start engine
cargo test -p iii-engine         # test engine
cargo test -p iii-sdk            # test Rust SDK
```

### Implementation Notes

- The engine is Rust, SDKs are TypeScript/Python/Rust. All three SDKs communicate with the engine over WebSocket.
- SDK naming: the npm/PyPI/crates.io package is `iii-sdk`. The import is `iii-sdk` (Node), `iii_sdk` (Python), `iii_sdk` (Rust).
- Engine config uses `expression` for cron schedule fields and leading slashes for `api_path` (e.g., `/orders`, not `orders`).
- The `skills/` directory contains agent skills following the [Agent Skills specification](https://agentskills.io/specification). Each skill has a `SKILL.md` with YAML frontmatter. Reference implementations live in `skills/references/` named after their skill.
- Skills are auto-discovered by `npx skills add iii-hq/iii` and `npx skillkit install iii-hq/iii` because SkillKit checks for a `skills/` directory at repo root.
- All skill folder names are iii-prefixed (e.g., `iii-channels/`, `iii-http-endpoints/`) for marketplace indexing. The `name` field in each `SKILL.md` matches the directory name.
- Console frontend is React. Console backend has a Rust component at `console/packages/console-rust/`.
- Motia is a higher-level framework built on top of iii-sdk, located at `frameworks/motia/`.
- Use `pnpm` (not npm) for all JS/TS dependency management. The lockfile is `pnpm-lock.yaml`.
- Use `cargo fmt --all` before committing Rust changes. Use `pnpm fmt` for JS/TS.
- Internal package references use `workspace:*` in pnpm (converted to actual versions on publish).

### Conventions

- Trigger config field for cron schedules: `expression` (not `cron`)
- HTTP trigger `api_path` values: always include leading slash (`/orders`, `/users/:id`)
- Function IDs use `::` separator for namespacing (e.g., `orders::validate`, `reports::daily-summary`)
- State scopes are named for the domain (e.g., `products`, `orders`, `sessions`)
- Queue names are descriptive (e.g., `fulfillment`, `notifications`)
- Skills must include "When to Use" and "Boundaries" sections in SKILL.md (SkillKit validates this)

### Licensing

- `engine/` — Elastic License v2 (ELv2)
- `sdk/`, `skills/`, `console/`, `frameworks/`, `docs/`, `website/` — Apache-2.0
