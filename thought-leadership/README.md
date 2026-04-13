# Thought leadership post assets (iii)

This folder holds research notes, architecture synthesis, a reference summary for an external piece, and a **plan + outline** for drafting a technical thought leadership article about **iii**.

## Files

| File | Purpose |
|------|---------|
| [`01-iii-architecture-deep-dive.md`](./01-iii-architecture-deep-dive.md) | How iii is structured (engine, modules, primitives, wire protocol, discovery) and how it compares to “many backends taped together.” |
| [`02-akshay-pachaar-agent-harness-reference.md`](./02-akshay-pachaar-agent-harness-reference.md) | Summary of the referenced X thread *The Anatomy of an Agent Harness* (original not directly fetchable here; verify against the live post). |
| [`03-community-implications.md`](./03-community-implications.md) | Implications for backend engineering and for agentic framework / harness communities; bridge concepts to iii. |
| [`04-post-plan-and-outline.md`](./04-post-plan-and-outline.md) | Workflow to produce the draft, audience, thesis options, section outline, and asset checklist. |
| [`05-harness-spectrum-and-iii-thesis.md`](./05-harness-spectrum-and-iii-thesis.md) | Harness spectrum (Anthropic/OpenAI/CrewAI/LangGraph), scaffolding metaphor, future-proofing test, and iii’s **infrastructure + primitives** thesis vs harness-thickness debate. |
| [`06-unified-worker-model-and-paradigm.md`](./06-unified-worker-model-and-paradigm.md) | **Everything is a worker** — engine features as composable workers, design patterns vs pillars, paradigm-shift framing. |

## How to use this

1. Read `01`, `03`, `05`, and `06` for technical + narrative grounding (harness debate vs execution layer vs unified worker composition).
2. Open the original X thread and reconcile any gaps with `02`.
3. Follow `04` to write the first draft (Markdown or Google Doc); pull diagrams from `docs/advanced/architecture.mdx` or recreate simplified versions.

## Source of truth

Product positioning and architecture details in this repo are aligned with:

- Root [`README.md`](../README.md)
- [`docs/index.mdx`](../docs/index.mdx)
- [`docs/advanced/architecture.mdx`](../docs/advanced/architecture.mdx)
- [`docs/primitives-and-concepts/discovery.mdx`](../docs/primitives-and-concepts/discovery.mdx)
- Skills such as [`skills/iii-agentic-backend/SKILL.md`](../skills/iii-agentic-backend/SKILL.md)
