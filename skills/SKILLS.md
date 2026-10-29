# iii Skills

Top-level skills for building on the [iii engine](https://iii.dev). This catalog stays compact on
purpose; worker-backed capability skills live with their worker docs and `workers.iii.dev`.

## Install

```bash
npx skills add iii-hq/iii/skills
```

## Catalog

- [iii-getting-started](iii-getting-started/SKILL.md): Install iii, create a project, write your first worker, and add registry workers
- [iii-core-primitives](iii-core-primitives/SKILL.md): Workers, functions, observers, calls and invocation modes, built-in source functions, registry access, channels, and HTTP-invoked functions
- [iii-observe](iii-observe/SKILL.md): `iii.observe(a, b)` (when `a` runs, `b` runs), HTTP, cron, queue, state, and engine sources, filters, event sources with `onObserverAdded` / `onObserverRemoved` / `onObserverDeliver`, local dispatch, and upgrading 0.25 code
- [iii-sdk-reference](iii-sdk-reference/SKILL.md): Node.js, browser, Python, Rust, and Go SDK usage in one place
- [iii-engine-config](iii-engine-config/SKILL.md): Configure engine ports, engine-owned workers, adapters, and RBAC via rbac-proxy
- [iii-architecture-patterns](iii-architecture-patterns/SKILL.md): Workflows, reactive backends, agentic pipelines, CQRS, effect pipelines, and automation chains
- [iii-error-handling](iii-error-handling/SKILL.md): Engine and SDK errors, retryability, RBAC denial, and timeout handling

## Tools

- [presentation](presentation/SKILL.md): Turn a tech-spec directory into an interactive, marketing-grade web presentation (ships a frozen Vite + React + TS scaffold and an interactive diagram archetype library)
