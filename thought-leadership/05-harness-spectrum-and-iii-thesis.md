# Harness spectrum, scaffolding, and iii’s thesis (author context)

This file captures **additional narrative context** for the thought leadership post: how major players frame the **agent harness**, where they **agree vs disagree**, and how **iii’s thesis** relates — especially **get the infrastructure right** and **core primitives as the differentiator** (not “thick vs thin harness” in the LLM loop sense).

## Shared premise (industry)

Anthropic, OpenAI, CrewAI, LangChain (and similar) all **build agents** and **wrap models in infrastructure** — each calls that wrapping a **harness**.

**Agreement:** the **model is not the product**. The **infrastructure around the model** is what ships.

**Disagreement:** **how much** of that infrastructure should exist — the central **architectural bet** in AI right now; each vendor places a different one.

## Spectrum: thin harness ↔ thick harness

| Actor | Bet (as framed in this narrative) |
|-------|-------------------------------------|
| **Anthropic** | Bets on the **model**. Harness is **deliberately thin**: a “dumb loop” — assemble prompt, call model, execute tool calls, repeat. Model decides; harness manages **turns**. Thesis: as models improve, **less** infrastructure, not more. |
| **OpenAI** | Similar but **slightly thicker**: Agents SDK is **code-first** (workflow logic in native code, not only a graph DSL). More structure: instruction stacks, orchestration modes, explicit handoffs. |
| **CrewAI** | **Deterministic backbone**: **Flows** for routing/validation (hard logic); **Crews** for autonomous parts. Intelligence where it matters; **control** elsewhere. |
| **LangGraph** | Bets on **explicit control**: harness **encodes** logic — nodes, edges, defined transitions. Planning/routing/multi-step workflows live in the harness, not only in the model. |

**One-line spectrum:**  
*Trust the model, keep the harness thin* ←————→ *Encode the logic, make the harness thick.*

## Scaffolding metaphor

- Scaffolding = **temporary** infrastructure so “workers” can reach what they couldn’t otherwise. It does not **do** the building; without it, they cannot reach upper floors.
- **Key word:** **temporary** — as the building rises, scaffolding **comes down**.
- **Examples in this narrative:** Manus rebuilding agents repeatedly; complexity removed over time (rich tools → shell commands; “management agents” → simple handoffs). Anthropic **removing** planning steps from Claude Code’s harness as new model versions absorb capability.

## The catch (coupling)

- Models are increasingly **trained with specific harnesses in the loop** — performance can **depend** on that scaffolding.
- **Change the scaffolding → performance can drop** — the “worker” learned **this** scaffolding.
- **Principle:** build scaffolding **designed to be removed** — but **remove carefully** because the model may have **learned to lean on it**.

## Future-proofing test (for agent systems)

> If **dropping in a more powerful model** improves performance **without adding harness complexity**, the design is sound.

**Corollary in this narrative:** two products with the **same model / weights** can perform **very differently** based on **harness thickness** — **infrastructure around the model** dominates outcomes.

## External claim to verify before publishing

- **LangChain / TerminalBench 2.0:** narrative claims **infrastructure-only** change (same model, same weights) moved ranking from **outside top 30** to **rank 5**.  
  - **Action:** confirm **benchmark name**, **version**, **date**, and **what exactly changed** (LangChain product vs internal stack) before citing in public.

## Link to “deep dive” article

- The author references a separate **deep dive on agent harness engineering** (orchestration loop, tools, memory, context management, etc.).  
- **Action:** add URL and title when publishing so the post can point readers to long-form detail.

## How iii fits (thesis — distinct from harness-thickness debate)

| Dimension | What the harness debate optimizes | What iii optimizes |
|-----------|-----------------------------------|----------------------|
| **Primary question** | How **thick** should the **LLM-facing** loop be? (turn management, graphs, flows, tool policy) | Is **backend execution** **correct, unified, and observable**? |
| **iii’s side** | (Does not pick Anthropic-vs-LangGraph for you.) | **Infrastructure wins** when invocation, triggers, queues, state, and discovery are **first-class** — *get that layer right*. |
| **Differentiator vs “cloud platforms” (e.g. Cloudflare in author framing)** | N/A | **Core primitives** — **Function, Trigger, Worker** — as the **unifying model**, not only “managed services stitched together.” **Verify** any named competitor framing against public positioning before print. |
| **Composition** | Features as separate pillars (queue SKU, stream SKU, …) | **Everything is a worker** — infrastructure pieces are **workers** in the same sense as SDK workers; see [`06-unified-worker-model-and-paradigm.md`](./06-unified-worker-model-and-paradigm.md). |

**Bridge sentence (drafting aid):**  
*The harness debate is about how much **cognitive** structure to encode. iii is about how much **execution** structure to unify — so whatever harness you choose, the **capabilities it calls** are stable, discoverable, and operable.*

## Vocabulary alignment with `02` / Akshay thread

- **Harness** = model-adjacent loop, tools, memory, context, guardrails.  
- **iii** = **execution substrate** under that: functions, triggers, workers, engine, live discovery — **orthogonal axis** to harness thickness.
