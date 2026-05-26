# iii Skills

[Agent Skills](https://agentskills.io) for building with the
[iii engine](https://github.com/iii-hq/iii) — functions, triggers, SDKs, engine configuration, and
iii architecture patterns.

Works with Claude Code, Cursor, Gemini CLI, OpenCode, Amp, Goose, Roo Code, GitHub Copilot, VS Code,
OpenAI Codex, and [30+ other agents](https://agentskills.io).

## Install

```bash
npx skills add iii-hq/iii/skills
```

### Install a single skill

```bash
npx skills add iii-hq/iii/skills --skill iii-functions-and-triggers
```

## Skills

### Getting Started

| Skill                                        | What it does                                           |
| -------------------------------------------- | ------------------------------------------------------ |
| [iii-getting-started](./iii-getting-started) | Install iii, create a project, write your first worker |

### HOWTO Skills

Direct mappings to [iii documentation](https://iii.dev/docs) HOWTOs. Each teaches an iii-level
primitive or engine concept. Worker-backed capabilities such as HTTP, cron, queue, pubsub, state,
streams, and observability live with the engine workers under `engine/src/workers/**/skills`.
Code examples live directly inside each skill.

| Skill                                                      | What it does                                                             |
| ---------------------------------------------------------- | ------------------------------------------------------------------------ |
| [iii-functions-and-triggers](./iii-functions-and-triggers) | Register functions and bind triggers across TypeScript, Python, and Rust |
| [iii-custom-triggers](./iii-custom-triggers)               | Build custom trigger types for external events                           |
| [iii-trigger-actions](./iii-trigger-actions)               | Synchronous, fire-and-forget, and enqueue invocation modes               |
| [iii-trigger-conditions](./iii-trigger-conditions)         | Gate trigger execution with condition functions                          |
| [iii-trigger-schemas](./iii-trigger-schemas)               | Built-in trigger config and handler payload schemas                      |
| [iii-engine-config](./iii-engine-config)                   | Configure the iii engine via iii-config.yaml                             |
| [iii-error-handling](./iii-error-handling)                 | Handle engine and SDK errors across languages                            |
| [iii-channels](./iii-channels)                             | Binary streaming between workers                                         |

### Architecture Pattern Skills

Compose multiple iii primitives into common backend architectures. Each includes direct code
examples in the skill.

| Skill                                                      | What it does                                               |
| ---------------------------------------------------------- | ---------------------------------------------------------- |
| [iii-agentic-backend](./iii-agentic-backend)               | Multi-agent pipelines with queue handoffs and shared state |
| [iii-reactive-backend](./iii-reactive-backend)             | Real-time backends with state triggers and stream updates  |
| [iii-workflow-orchestration](./iii-workflow-orchestration) | Durable multi-step pipelines with retries and DLQ          |
| [iii-http-invoked-functions](./iii-http-invoked-functions) | Register external HTTP endpoints as iii functions          |
| [iii-effect-system](./iii-effect-system)                   | Composable, traceable function pipelines                   |
| [iii-event-driven-cqrs](./iii-event-driven-cqrs)           | CQRS with event sourcing and independent projections       |
| [iii-low-code-automation](./iii-low-code-automation)       | Trigger-transform-action automation chains                 |

### SDK Reference Skills

| Skill                                | What it does                     |
| ------------------------------------ | -------------------------------- |
| [iii-node-sdk](./iii-node-sdk)       | Node.js/TypeScript SDK reference |
| [iii-browser-sdk](./iii-browser-sdk) | Browser SDK reference            |
| [iii-python-sdk](./iii-python-sdk)   | Python SDK reference             |
| [iii-rust-sdk](./iii-rust-sdk)       | Rust SDK reference               |

## Format

Each skill follows the [Agent Skills specification](https://agentskills.io/specification):

```text
skills/
├── iii-functions-and-triggers/
│   └── SKILL.md                # YAML frontmatter (name + description) + markdown instructions
├── iii-channels/
│   └── SKILL.md
└── README.md
```

Skills are activated automatically when the agent detects a matching task based on the description
field. Code examples are embedded directly in the matching `SKILL.md`.

## Contributing

1. Fork this repo
2. Add or edit a skill in `skills/`
3. Submit a PR

## License

Apache-2.0
