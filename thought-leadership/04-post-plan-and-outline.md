# Plan and outline: technical thought leadership post on iii

## Goal

Publish a **technical** (not purely promotional) article that:

1. Explains **iii’s architecture** at a level credible to senior backend and ML-platform readers.  
2. Connects to **broader trends**: fragmented backend stacks, polyglot orgs, agent harnesses, discoverable capabilities.  
3. **Engages** [Akshay Pachaar’s thread *The Anatomy of an Agent Harness*](https://x.com/akshay_pachaar/status/2041146899319971922) as a **reference frame** (after verifying details on X).  
4. Optionally weaves the **harness spectrum** (Anthropic / OpenAI / CrewAI / LangGraph — thin vs thick **LLM** harness) from [`05-harness-spectrum-and-iii-thesis.md`](./05-harness-spectrum-and-iii-thesis.md), and positions iii on the **orthogonal** axis: **execution infrastructure** + **core primitives**.  
5. Leaves readers with a **clear mental model**: Function / Trigger / Worker + engine + live discovery.

## Audience

- **Primary:** Staff+/principal backend engineers, platform engineers, ML/AI infrastructure engineers building agents.  
- **Secondary:** Technical leads evaluating orchestration / “unified runtime” stories.

## Recommended workflow (uses assets in this directory)

| Step | Action | Output |
|------|--------|--------|
| 1 | Finalize **one thesis line** from `03-community-implications.md` | Single sentence at top of draft |
| 2 | Re-read **`01-iii-architecture-deep-dive.md`** and skim **`docs/advanced/architecture.mdx`** | Accurate diagrams / no overclaims |
| 3 | Open the **X thread**; update **`02-akshay-pachaar-agent-harness-reference.md`** with anything you want to quote | Clean attribution |
| 4 | Draft **section 1–2** (hook + problem) | 600–900 words |
| 5 | Draft **section 3–4** (iii model + discovery) | Include one diagram (mermaid or exported figure) |
| 6 | Draft **section 5** (agents / harness bridge) | Explicit “what iii does / does not do” |
| 7 | Draft **closing** (who should adopt, when not to) | Honest boundaries |
| 8 | **Tech review** against engine/docs (optional: internal reviewer) | Factual pass |
| 9 | **Copy edit** for voice (confident, precise, minimal hype) | Final post |

**Length target:** ~1,800–2,800 words for a strong web piece; shorter if this is X/Twitter long-form or LinkedIn.

## Outline (suggested)

### Title options (working)

- *Beyond the harness: a unified execution layer for everything agents must call*  
- *Function, trigger, worker: a backend mental model for the agent era*  
- *What agent harnesses still cannot fix (and what a unified engine changes)*  

*(Pick after thesis is fixed.)*

### 1. Hook (≈ 2–3 short paragraphs)

- Open with the **agent harness** idea: cognition wrapped in tools, memory, loops — **quoting or paraphrasing** the Akshay thread after verification.  
- Optional tightening: **everyone** building agents agrees the **model isn’t the product** — the **infrastructure around it** is; they **disagree** how **thick** that harness should be (see `05` for spectrum).  
- Pivot: that debate is **necessary but incomplete** — **tool execution, durability, and discovery** still need a **home**; scaffolding can shrink **up the stack** while **distributed execution** remains.  
- Secondary pivot: hardest failures are often **systems** — **invocation, durability, cross-service composition** — not the next 3% on a benchmark.

### 2. The fragmented backend (problem)

- The “six tools” story from [`README.md`](../README.md) / [`docs/index.mdx`](../docs/index.mdx): API, queue, cron, bus, state, observability — **integration tax**.  
- Why this hurts **velocity** and **debuggability**, and why it **multiplies** when agents generate more invocations.

### 3. iii’s model (solution — architecture)

- **Engine + modules + workers** — one paragraph each.  
- **Critical differentiator:** **everything in the engine is a worker** — queue, stream, HTTP, cron, state, … are **workers**, not separate product pillars; **your** code also attaches as **workers**. Capabilities = **design patterns**, not a feature checklist — see `06`.  
- **Function / Trigger / Worker** — crisp definitions; one **concrete** example (HTTP trigger → handler → `trigger()` to another function).  
- **Wire protocol** — WebSocket, JSON, polyglot — **one sentence** on why it matters for orgs with mixed stacks.

### 4. Live discovery (differentiator)

- Pull from **`docs/primitives-and-concepts/discovery.mdx`**: push-based registry, contrast with DNS/Consul/mesh **at a high level** (no straw-man).  
- Tie to **agents**: capability sets that **update live** as workers connect.

### 5. Bridge to harness / agent frameworks (two layers)

- **Layer A — LLM harness (vendor-specific):** thickness of loop, graphs, flows, handoffs — *pick your bet* (thin vs thick).  
- **Layer B — execution substrate:** what actually runs when a tool fires — **iii’s** story: Function / Trigger / Worker, engine, discovery, OTel.  
- Explicit: iii does **not** require choosing Anthropic’s loop over LangGraph’s; it **grounds** whatever harness you use.  
- **Differentiator sentence:** iii is **primitive-centric** (three primitives unify the backend), not “only” bundling adjacent cloud services — contrast **carefully** with platforms like Cloudflare if named (**verify** claims).  
- Optional: nod to **Motia** as higher-level framework; **SkillKit** as agent-facing docs — **brief**.  
- Optional: **scaffolding** + **future-proofing test** from `05` — if citing **LangChain / TerminalBench**, **verify** numbers and setup first.

### 6. When iii fits / when it does not

- Fits: polyglot backends, unified ops story, agent tool routing, observable pipelines.  
- Does not replace: ML training, safety policy engines, pure data warehouses — **be explicit**.

### 7. Close

- One **memorable analogy** (React for UI → iii for backend is already in docs — use only if it feels fresh).  
- **CTA**: link to quickstart, GitHub, docs — single line.

## Assets checklist (reuse when drafting)

- [ ] One **architecture diagram** (engine ↔ modules ↔ workers) — from docs or simplified.  
- [ ] One **sequence** (HTTP → engine → worker) optional.  
- [ ] **Glossary** inline or footnote: Function ID, Trigger types at high level.  
- [ ] **Verified** quotes from the Akshay thread (if any).  

## Files in this folder to keep updated as you draft

| File | Update when… |
|------|----------------|
| `01-iii-architecture-deep-dive.md` | You add benchmarks, customer stories, or new modules — keep narrative aligned with product. |
| `02-akshay-pachaar-agent-harness-reference.md` | You capture exact wording from X. |
| `03-community-implications.md` | You choose final thesis and audience. |
| `04-post-plan-and-outline.md` | You lock outline sections and word count. |

## Optional next assets (not created yet)

- `draft-v1.md` — first full draft (add when ready).  
- `quotes-and-citations.md` — external links, talk titles, papers (if you expand scope).  
- `diagrams/` — exported PNG/SVG from mermaid for blog CMS.
