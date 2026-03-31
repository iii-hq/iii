# iii Skills

[Agent Skills](https://agentskills.io) for building with the [iii engine](https://github.com/iii-hq/iii) — functions, triggers, workers, state, streams, and more.

Works with Claude Code, Cursor, Gemini CLI, OpenCode, Amp, Goose, Roo Code, GitHub Copilot, VS Code, OpenAI Codex, and [30+ other agents](https://agentskills.io).

## Install

### One command

```bash
npx skills add iii-hq/iii
```

### SkillKit

```bash
# Install all iii skills
npx skillkit install iii-hq/iii

# Install a single skill
npx skillkit install iii-hq/iii --skills=iii-http-endpoints

# Sync skills across all your agents
npx skillkit sync
```

### Git clone

```bash
# Claude Code
git clone https://github.com/iii-hq/iii.git /tmp/iii && cp -r /tmp/iii/skills/iii-* ~/.claude/skills/

# Cursor
git clone https://github.com/iii-hq/iii.git /tmp/iii && cp -r /tmp/iii/skills/iii-* ~/.cursor/skills/

# Gemini CLI
git clone https://github.com/iii-hq/iii.git /tmp/iii && cp -r /tmp/iii/skills/iii-* ~/.gemini/skills/
```

### Multi-agent sync

If you use multiple agents, SkillKit keeps skills in sync across all of them:

```bash
# Install once, sync to Claude Code + Cursor + Gemini CLI
npx skillkit install iii-hq/iii
npx skillkit sync --agent claude-code
npx skillkit sync --agent cursor
npx skillkit sync --agent gemini-cli
```

Supports 32+ agents including Claude Code, Cursor, Codex, Gemini CLI, OpenCode, Amp, Goose, Roo Code, GitHub Copilot, and more.

## Skills

### Getting Started

| Skill | What it does |
| --- | --- |
| [iii-getting-started](./iii-getting-started) | Install iii, create a project, write your first worker |

### HOWTO Skills

Direct mappings to [iii documentation](https://iii.dev/docs) HOWTOs. Each teaches one primitive or capability. Reference implementations are available in TypeScript, Python, and Rust.

| Skill | What it does |
| --- | --- |
| [iii-functions-and-triggers](./iii-functions-and-triggers) | Register functions and bind triggers across TypeScript, Python, and Rust |
| [iii-http-endpoints](./iii-http-endpoints) | Expose functions as REST API endpoints |
| [iii-cron-scheduling](./iii-cron-scheduling) | Schedule recurring tasks with cron expressions |
| [iii-queue-processing](./iii-queue-processing) | Async job processing with retries, concurrency, and ordering |
| [iii-state-management](./iii-state-management) | Distributed key-value state across functions |
| [iii-state-reactions](./iii-state-reactions) | Auto-trigger functions on state changes |
| [iii-realtime-streams](./iii-realtime-streams) | Push live updates to WebSocket clients |
| [iii-custom-triggers](./iii-custom-triggers) | Build custom trigger types for external events |
| [iii-trigger-actions](./iii-trigger-actions) | Synchronous, fire-and-forget, and enqueue invocation modes |
| [iii-trigger-conditions](./iii-trigger-conditions) | Gate trigger execution with condition functions |
| [iii-dead-letter-queues](./iii-dead-letter-queues) | Inspect and redrive failed queue jobs |
| [iii-engine-config](./iii-engine-config) | Configure the iii engine via iii-config.yaml |
| [iii-observability](./iii-observability) | OpenTelemetry tracing, metrics, and logging |
| [iii-channels](./iii-channels) | Binary streaming between workers |

### Architecture Pattern Skills

Compose multiple iii primitives into common backend architectures. Each includes a full working reference implementation.

| Skill | What it does |
| --- | --- |
| [iii-agentic-backend](./iii-agentic-backend) | Multi-agent pipelines with queue handoffs and shared state |
| [iii-reactive-backend](./iii-reactive-backend) | Real-time backends with state triggers and stream updates |
| [iii-workflow-orchestration](./iii-workflow-orchestration) | Durable multi-step pipelines with retries and DLQ |
| [iii-http-invoked-functions](./iii-http-invoked-functions) | Register external HTTP endpoints as iii functions |
| [iii-effect-system](./iii-effect-system) | Composable, traceable function pipelines |
| [iii-event-driven-cqrs](./iii-event-driven-cqrs) | CQRS with event sourcing and independent projections |
| [iii-low-code-automation](./iii-low-code-automation) | Trigger-transform-action automation chains |

### SDK Reference Skills

| Skill | What it does |
| --- | --- |
| [iii-node-sdk](./iii-node-sdk) | Node.js/TypeScript SDK reference |
| [iii-python-sdk](./iii-python-sdk) | Python SDK reference |
| [iii-rust-sdk](./iii-rust-sdk) | Rust SDK reference |

### Shared References

| File | What it contains |
| --- | --- |
| [references/iii-config.yaml](./references/iii-config.yaml) | Full annotated engine configuration reference |

## Format

Each skill follows the [Agent Skills specification](https://agentskills.io/specification):

```text
skills/
├── iii-http-endpoints/
│   └── SKILL.md                # YAML frontmatter (name + description) + markdown instructions
├── iii-channels/
│   └── SKILL.md
├── references/
│   ├── http-endpoints.js       # TypeScript reference implementation
│   ├── http-endpoints.py       # Python reference implementation
│   ├── http-endpoints.rs       # Rust reference implementation
│   ├── iii-config.yaml         # Shared engine config reference
│   └── ...
└── README.md
```

Skills are activated automatically when the agent detects a matching task based on the description field. Code references live in the `references/` directory, named after their skill.

## Contributing

1. Fork this repo
2. Add or edit a skill in `skills/`
3. Submit a PR

## License

Apache-2.0
