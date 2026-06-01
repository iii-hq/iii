# iii Skills

Top-level skills for building with the [iii engine](https://github.com/iii-hq/iii). The catalog is
intentionally small: it teaches the iii mental model, worker creation and registry access, SDK
usage, engine config, architecture patterns, and error handling. Worker-backed capability skills
stay with the worker docs and [workers.iii.dev](https://workers.iii.dev/) instead of duplicating
queue, pub/sub, state, cron, stream, or observability here.

## Install

```bash
npx skills add iii-hq/iii/skills
```

### Install a single skill

```bash
npx skills add iii-hq/iii/skills --skill iii-core-primitives
```

## Skills

| Skill                                                  | What it does |
| ------------------------------------------------------ | ------------ |
| [iii-getting-started](./iii-getting-started)           | Install iii, create a project, write your first worker, and add registry workers |
| [iii-core-primitives](./iii-core-primitives)           | Functions, triggers, workers, registry access, invocation modes, trigger schemas, custom triggers, channels, and HTTP-invoked functions |
| [iii-sdk-reference](./iii-sdk-reference)               | Node.js, browser, Python, and Rust SDK usage in one place |
| [iii-engine-config](./iii-engine-config)               | Configure ports, workers, adapters, queues, worker manager, RBAC, and observability |
| [iii-architecture-patterns](./iii-architecture-patterns) | Workflows, reactive backends, agentic pipelines, CQRS, effect pipelines, and automation chains |
| [iii-error-handling](./iii-error-handling)             | Engine and SDK errors, retryability, RBAC denial, and timeout handling |

## Shape

Each skill is one folder with one `SKILL.md`. Code examples live directly in the skill, including
TypeScript, Python, and Rust examples where the concept is language-specific.

```text
skills/
├── iii-core-primitives/
│   └── SKILL.md
├── iii-sdk-reference/
│   └── SKILL.md
└── README.md
```

## Contributing

1. Fork this repo
2. Add or edit a top-level skill in `skills/`
3. Keep worker-specific capability skills with the worker docs
4. Submit a PR

## License

Apache-2.0
