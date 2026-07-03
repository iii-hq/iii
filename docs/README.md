---
title: "iii Docs"
description: "A high-level overview of iii and how to work with the documentation in this directory."
owner: "devrel"
type: "explanation"
---

# iii Docs

iii is a software system that eliminates integration complexity. It organizes software into three
primitives (Worker, Trigger, Function), so every capability (queues, cron, streaming, sandboxing,
observability, agents, frontend UIs) can be built and composed from the same set of pieces. The
rendered documentation site is the canonical reference; this directory holds the Mintlify source.

## Development

From the repository root:

```bash
pnpm dev:docs
```

Or directly from this directory:

```bash
npx mint dev
```

The docs config is in `docs.json`. Local preview is typically available at `http://localhost:3000`.

## API Reference

To refresh generated SDK API reference files before previewing docs:

```bash
pnpm generate:api-docs
```

## Releasing

The docs are versioned with a Next / Latest / archive scheme. To author upcoming docs in `docs/next/`
and understand how a release promotes them into the published Latest, see
[RELEASING.md](./RELEASING.md).
