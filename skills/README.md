# iii Skills

[Agent Skills](https://agentskills.io) for building with the [iii engine](https://github.com/iii-hq/iii) — functions, triggers, workers, state, streams, and more.

Works with Claude Code, Cursor, Gemini CLI, OpenCode, Amp, Goose, Roo Code, GitHub Copilot, VS Code, OpenAI Codex, and [30+ other agents](https://agentskills.io).

## Install

### One command

```bash
npx skills add iii-hq/skills
```

### SkillKit

```bash
# Install all iii skills
npx skillkit install iii-hq/skills

# Install a single skill
npx skillkit install iii-hq/skills --skills=http-endpoints

# Sync skills across all your agents
npx skillkit sync
```

### Git clone

```bash
# Claude Code
git clone https://github.com/iii-hq/skills.git ~/.claude/skills/iii

# Cursor
git clone https://github.com/iii-hq/skills.git ~/.cursor/skills/iii

# Gemini CLI
git clone https://github.com/iii-hq/skills.git ~/.gemini/skills/iii
```

### Multi-agent sync

If you use multiple agents, SkillKit keeps skills in sync across all of them:

```bash
# Install once, sync to Claude Code + Cursor + Gemini CLI
npx skillkit install iii-hq/skills
npx skillkit sync --agent claude-code
npx skillkit sync --agent cursor
npx skillkit sync --agent gemini-cli

# Or translate all skills to a specific agent
npx skillkit translate --all --to cursor
```

Supports 32+ agents including Claude Code, Cursor, Codex, Gemini CLI, OpenCode, Amp, Goose, Roo Code, GitHub Copilot, and more.

## Skills

### HOWTO Skills

Direct mappings to [iii documentation](https://iii.dev/docs) HOWTOs. Each teaches one primitive or capability. Reference implementations are available in TypeScript, Python, and Rust.

| Skill                                              | What it does                                                             |
| -------------------------------------------------- | ------------------------------------------------------------------------ |
| [functions-and-triggers](./iii-functions-and-triggers) | Register functions and bind triggers across TypeScript, Python, and Rust |
| [http-endpoints](./iii-http-endpoints)                 | Expose functions as REST API endpoints                                   |
| [cron-scheduling](./iii-cron-scheduling)               | Schedule recurring tasks with cron expressions                           |
| [queue-processing](./iii-queue-processing)             | Async job processing with retries, concurrency, and ordering             |
| [state-management](./iii-state-management)             | Distributed key-value state across functions                             |
| [state-reactions](./iii-state-reactions)               | Auto-trigger functions on state changes                                  |
| [realtime-streams](./iii-realtime-streams)             | Push live updates to WebSocket clients                                   |
| [custom-triggers](./iii-custom-triggers)               | Build custom trigger types for external events                           |
| [trigger-actions](./iii-trigger-actions)               | Synchronous, fire-and-forget, and enqueue invocation modes               |
| [trigger-conditions](./iii-trigger-conditions)         | Gate trigger execution with condition functions                          |
| [dead-letter-queues](./iii-dead-letter-queues)         | Inspect and redrive failed queue jobs                                    |
| [engine-config](./iii-engine-config)                   | Configure the iii engine via iii-config.yaml                             |
| [observability](./iii-observability)                   | OpenTelemetry tracing, metrics, and logging                              |
| [channels](./iii-channels)                             | Binary streaming between workers                                         |

### Architecture Pattern Skills

Compose multiple iii primitives into common backend architectures. Each includes a full working reference implementation.

| Skill                                              | What it does                                               |
| -------------------------------------------------- | ---------------------------------------------------------- |
| [agentic-backend](./iii-agentic-backend)               | Multi-agent pipelines with queue handoffs and shared state |
| [reactive-backend](./iii-reactive-backend)             | Real-time backends with state triggers and stream updates  |
| [workflow-orchestration](./iii-workflow-orchestration) | Durable multi-step pipelines with retries and DLQ          |
| [http-invoked-functions](./iii-http-invoked-functions) | Register external HTTP endpoints as iii functions          |
| [effect-system](./iii-effect-system)                   | Composable, traceable function pipelines                   |
| [event-driven-cqrs](./iii-event-driven-cqrs)           | CQRS with event sourcing and independent projections       |
| [low-code-automation](./iii-low-code-automation)       | Trigger-transform-action automation chains                 |

### SDK Reference Skills

| Skill                      | What it does                     |
| -------------------------- | -------------------------------- |
| [node-sdk](./iii-node-sdk)     | Node.js/TypeScript SDK reference |
| [python-sdk](./iii-python-sdk) | Python SDK reference             |
| [rust-sdk](./iii-rust-sdk)     | Rust SDK reference               |

### Shared References

| File                                                       | What it contains                                                      |
| ---------------------------------------------------------- | --------------------------------------------------------------------- |
| [references/iii-config.yaml](./references/iii-config.yaml) | Full annotated engine configuration reference (auto-synced from docs) |

## Format

Each skill follows the [Agent Skills specification](https://agentskills.io/specification):

```text
iii-http-endpoints/
└── SKILL.md                    # YAML frontmatter (name + description) + markdown instructions

references/
├── http-endpoints.js           # TypeScript reference implementation
├── http-endpoints.py           # Python reference implementation
├── http-endpoints.rs           # Rust reference implementation
├── iii-config.yaml             # Shared engine config reference
└── ...
```

Skills are activated automatically when the agent detects a matching task based on the description field. Code references live in the root `references/` directory, named after their skill.

## Contributing

1. Fork this repo
2. Add or edit a skill in its own folder
3. Submit a PR

## License

Apache-2.0
