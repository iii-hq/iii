---
title: 'How to diagnose context rot in long-running AI agents'
seoTitle: 'How to Diagnose Context Rot in AI Agents'
description: 'Diagnose context rot in long-running AI agents with 4 practical tests. Use iii traces and session replays to find the cause and choose the right fix.'
pubDate: 2026-11-30
tags: ['agents', 'context', 'harness', 'observability', 'tracing']
ogImage: ../../assets/blog/diagnose-context-rot/og.png
faq:
  - q: 'Does a bigger context window fix context rot?'
    a: 'A larger context window delays overflow and gives the model more room before truncation kicks in, but it does not guarantee that the model will use long inputs accurately. Du et al. found performance degradation within tested models'' advertised limits, so the problem can appear well before you hit the ceiling. Anthropic''s context engineering guidance makes the same point: filling a large window with low-signal content can be worse than a smaller, well-curated context.'
  - q: 'What is an example of context collapse?'
    a: 'Context collapse is what happens when compaction discards detail that the agent still needs. After 2 compaction rounds in a 40-issue triage session, the summary might retain the final label decisions but lose the record of which issues had already received comments, so the agent re-comments on issues it already addressed. The fix is to include explicit preservation instructions in the compaction guidance and to verify what the summary retains before continuing the session.'
  - q: 'Does agent memory make context rot worse?'
    a: 'It can, when a memory worker adds outdated facts to every turn. If a memory worker injected a rule or entity state from an earlier session, that content arrives in every assembled context regardless of whether it is still accurate. In iii, memory::preview previews the rules and memories that will be injected before a turn runs. Capture the actual context::assemble output for the bad turn to verify what the model received, because the preview and the final assembled input can differ if hooks modify the context after assembly.'
---

![Diagnosing context rot: the assembled context grows turn by turn until the first bad turn.](../../assets/blog/diagnose-context-rot/banner.png)

Imagine an agent labeling GitHub issues. It follows your detailed instructions for the first 10 issues, then stops applying a particular rule.

The rule is still in the prompt, but the session has grown, so it dropped from the model's context.

The usual response is to add more context, switch to a larger model, or swap [AI Agent Harnesses](https://iii.dev/blog/the-harness-is-the-backend/). That sequence is expensive and rarely targets the actual problem. Before doing any of it, you need to know whether the issue is the context itself, a tool failure, a change in the retrieved information, or a task the model never understood clearly enough to follow consistently.

[iii](https://iii.dev/blog/the-substrate-is-installable/) records what the agent sent to the model on each turn, with token usage, and you can fork a session to replay a single turn in isolation.

This guide covers what context rot is, the warning signs, 4 tests, our test results, and how to match a remedy to each cause. We ran a 40-issue triage test on iii engine 0.26 with AI Agent Harness 1.9.2, context-manager 1.2.0, and session-manager 1.1.0.

## What context rot is

Context rot is a decline in LLM performance when processing long inputs. [Chroma measured this across 18 models](https://www.trychroma.com/research/context-rot), finding accuracy decline with longer inputs even when the retrieval itself succeeded. [Du et al. (October 2025)](https://arxiv.org/abs/2510.05381) extended this finding, showing that performance could deteriorate with longer inputs even when retrieval succeeded and the content fell within the tested models' advertised limits.

3 problems look similar but have different causes.

A full context window usually triggers an error, truncation, or compaction by the AI Agent Harness before the model ever sees the input. Lost in the middle is a position effect described by [Liu et al.](https://arxiv.org/abs/2307.03172), where a fact's position in a long input affects how reliably the model uses it, independent of whether the fact is present.

Multi-turn drift has a different cause. [Laban et al.](https://arxiv.org/abs/2505.06120) found a 39% relative average task-performance loss with underspecified multi-turn instructions. Early wrong guesses persisted through the session, driving the loss rather than context length alone.

Degradation varies by model family, task type, and context content. It need not be gradual and there is no single token count where problems reliably start.

Chroma's data shows each model family degrades in its own way, so a threshold that holds for one model tells you nothing about another.

## Signs that point to context rot vs signs that point elsewhere

Before running any tests, use the table below to narrow down where to start.

| Symptom | Possible cause | Check first |
| --- | --- | --- |
| Ignores an early instruction that is still in the context | Context rot | Confirm the instruction was in the model input at the bad turn |
| Uses a value that was corrected later in the session | Conflicting or stale context | Look for both the old and corrected values in the assembled input, and check whether a memory worker injected the outdated one |
| Repeats earlier actions or loops | Context rot or a weak stop condition | Replay the turn with a focused context (test 3) |
| Needs a fact absent from the model input | Retrieval or context-assembly change | Search the turn's input for the fact before assuming the model failed to use it |
| Acts on an error, empty, or truncated tool result | Tool error | Check the function span's status and output size |
| Fails in the first few turns, or differently on every run | Task ambiguity or run variability | Rerun with a short context and a clearer task before investigating context length |

## How to diagnose context rot in 4 tests

### Test 1: Find the first bad turn and what the model saw

**What to check:** Identify the first turn where the agent's output drifts from the task. Record its input token count. Then check whether the fact or instruction the agent should have followed was actually present in the assembled input at that turn.

**What it tells you:** If the fact was present but the model ignored it, context degradation could explain the failure. If the fact was missing, the problem could be retrieval, pruning, compaction, or context assembly, any of which can drop content before it reaches the model. A missing fact redirects the investigation rather than confirming context rot.

In iii, we do this by opening the Traces screen and finding the `router::chat` span for the bad turn. The span records what was sent to the model and the reported token usage. Cross-reference it with the `context::assemble` result, which runs before any `harness::hook::on-pre-generate` hooks fire. If a hook modifies the context after assembly, the trace records those changes separately, so verify that the captured request reflects the final post-hook state rather than the pre-hook draft.

**Limitation:** Token counts in the trace are estimates until the provider reports actual usage. iii's context-manager uses local estimation for assembly decisions, and exact counts come from the provider's response. Trace payloads can also be size-capped at the engine level, so a large assembled context may appear truncated in the trace view even when the full payload was sent.

### Test 2: Rule out the tool

**What to check:** For every function call that preceded the bad turn, inspect the span's status, its output size, and its content. Pay attention to capped or pruned placeholders, which indicate that the original result exceeded the `max_result_tokens` limit and was truncated before the model saw it.

**What it tells you:** An error, an empty result, or a truncated result is a separate candidate cause. It doesn't rule out context degradation, but it means you should fix the tool issue before drawing conclusions about context. A successful status alone is not enough: the result can still contain misleading information.

Inside iii, Function spans record status and output size. When context-manager's `context::assemble` caps an oversized result, it replaces the content with a bounded note in the form `[…result capped: was ~N tokens; middle omitted; re-call {function_id} with narrower arguments if the omitted middle is needed]`. That placeholder is what the model receives. Retry logic, schema validation, and provider-side errors can also affect the result, but debugging them is outside this guide's scope.

**Limitation:** A result that looks valid in the span can still mislead the model if its content is ambiguous or stale. Read the content, not the status code.

### Test 3: Replay the turn with a focused context

**What to check:** Fork the session at the point just before the bad turn, then run 2 versions, one with the full session history and one with only the original task, the relevant rules, and the most recent tool results needed for that turn.

**What it tells you:** If the focused replay produces the correct output and the full-history replay does not, the accumulated context contributes to the failure. This does not isolate length as the cause. The focused context also removes retrieval burden, distracting content, and stale tool results. Chroma used a similar comparison: focused inputs averaging around 300 tokens versus full histories averaging around 113k tokens. If both replays fail, investigate task ambiguity or tool issues before assuming context is the problem.

To do this in iii, you can use `session::fork`, which copies the session transcript to a new session. The fork is a separate session, not a child of the original. Its metrics count only model calls made within the fork; the original session's usage stays separate. Measure the replay's token counts as new calls.

**Limitation:** A session fork does not reset external state. If the bad turn depended on a specific GitHub issue state, repository labels, or any external resource that changed during the original session, the replay operates against the current external state, not the state at the time of the original turn. Run replays with matched tool state wherever possible.

### Test 4: Repeat the run to separate rot from noise

**What to check:** Run the same task a few times with identical settings. Record the first bad turn and the input token count at that turn for each run, or note that no failure occurred.

**What it tells you:** Repeated failures occurring at similar token sizes suggest a pattern worth investigating. They don't prove that length caused the failure, only that something is consistent across runs at that context size. Failures that arrive early or at different points on each run are more likely to reflect task ambiguity or run variability than context degradation.

Keep the model, reasoning setting, and agent profile identical across all the runs in iii. `harness::metrics` aggregates total token usage and an [estimated cost](https://iii.dev/blog/coding-agent-cost/) per model across a session, so you can compare token growth across runs at a glance.

**Limitation:** 4 to 5 runs are enough to spot a pattern, but not enough to rule out coincidence. [Laban et al.](https://arxiv.org/abs/2505.06120) documented unreliability and early wrong assumptions as a failure mode in multi-turn settings. Consider whether your task gives the model enough information on the first turn to stay on track across runs.

## What we found when we ran it in iii

We ran a [triage agent](https://iii.dev/blog/building-agents-for-real-world/) against 40 GitHub issues in a single session, applying labels and comments according to a rule set that included a "needs-reproduction" label for any issue without a reproducible test case. Each issue had a padded "Related discussion" thread so tool results had real bulk to manage.

We used iii engine 0.26.0, AI Agent Harness 1.9.2, context-manager 1.2.0, and session-manager 1.1.0. The agent profile was fixed across all conditions. We ran deepseek-flash at a fixed `thinking_level` of low throughout. Issues were processed in a fixed order. Repository state was reset between runs using the GitHub REST API.

deepseek-flash advertises a 1,000,000-token context window, and a complete 40-issue run only consumes 40,000 to 50,000 tokens against it, nowhere close to the limit. To get a meaningful comparison at this scale, we capped every run with the same `max_total_tokens` (70,000) and `max_output_tokens` (1,500) through the options of `harness::send`.

In practice this behaves like a synthetic smaller context window rather than a spending cap: the harness reserves the turn's full current context size plus `max_output_tokens` against the budget before every call, so a run fails once its accumulated context approaches the cap, regardless of how cheaply the provider's cache served it.

We measured how many of the 40 issues each condition's context-management strategy let the agent complete correctly before that shared cap was reached.

Context-manager was configured with `decay_user_turns` (0, disabling automatic decay), `protect_recent_tokens` (40,000), and `max_result_tokens` (20,000). The baseline therefore reflects pruning and capping as context-manager applies them by default, based on unmanaged history. We tested 4 conditions, with 5 runs each:

**Baseline:** The AI Agent Harness applied its default context-management settings with no additional pruning or compaction beyond automatic caps.

**Trim old tool results:** We set `decay_user_turns` to 2 and lowered `protect_recent_tokens` to 6,000 so `context::prune` would replace eligible old function outputs with placeholders well before the shared budget was exhausted, rather than relying on the much larger default protection window. No model call is involved, and context-manager substitutes the content with `[output of {function_id} pruned: was ~N tokens; re-call it if still needed]`. But aggressive trimming may remove a result the model still needs, forcing another tool call and adding to the context.

**Fetch the rule when needed:** We moved the labeling rule out of the task instructions and fetched it via a function call before each issue's labeling decision. This is just-in-time context: reading current state through a function rather than relying on the transcript to carry it forward. However, for caching providers like DeepSeek, every issue adds a tool round trip that may cost more per issue than paying once for a rule embedded in the task instructions and reading it from cache afterward.

**Summarize with /compact:** We processed the 40 issues in 4 batches of 10 and triggered `/compact keep the ticket ids` between batches. The `/compact` command runs server-side through the AI Agent Harness. Context-manager summarizes the older conversation, keeps the recent turns verbatim, and returns a compaction marker visible in the session. Although compaction can genuinely keep the context smaller, the summarization calls and the cache resets they trigger may cost more than that smaller context saves.

<figure>
  <img src="../../assets/blog/diagnose-context-rot/compact-summary.png" alt="A compaction summary in the iii console, headed COMPACTED 7,902 TOKENS, with Goal, Constraints and Preferences, Progress, and Key Decisions sections listing each triaged issue and whether the needs-reproduction label was applied." width="732" height="1047" />
  <figcaption>The summary iii's context-manager produced after <code>/compact keep the ticket ids</code> ran mid-session. It records the issue decisions and their reasons so the agent can keep track after older turns are rewritten.</figcaption>
</figure>

**Note:** These results apply to this test. A different model, thinking level, set of tools, or task may produce very different results. Run the tests on your own workload to choose an approach.

| Condition | Runs passed (of 5) | Median first bad issue | Context size at first bad turn | Total tokens (median) | Estimated cost (median) |
| --- | --- | --- | --- | --- | --- |
| Baseline | 0 | Issue 32 | ~33,700 | ~30,800 | $0.0203 |
| Trim old tool results | 1 | Issue 34 | ~30,400 | ~31,200 | $0.0204 |
| Fetch the rule when needed | 3 | Issue 37 | ~34,000 | ~30,500 | $0.0202 |
| Summarize with /compact | 2 | Issue 34 | ~17,900 | ~48,700 | $0.0267 |

Total token and cost figures are medians across all 5 runs per condition, including failed runs. "Context size at first bad turn" is the fresh input tokens plus cache-read tokens on the last turn completed before the run failed or the shared budget ran out, which tracks total prompt size more closely than fresh input tokens alone once prompt caching is in play.

0 of the 20 runs completed all 40 issues within the shared budget. Cost is estimated from provider-reported token counts, not from charged invoices.

[Install iii](https://iii.dev/docs/install) and open Traces to inspect the `router::chat` span and the token usage reported for each turn in your own agent run, then use `session::fork` before the first bad turn to replay it with a focused context.

<figure data-highlight>
  <img src="../../assets/blog/diagnose-context-rot/session-fork-replay.png" alt="A forked triage session in the iii console: the copied transcript starts with the 40-issue task prompt, shows the agent triggering 78 functions, then continues from a new instruction to process issue 37 next, ending with DONE." width="994" height="1034" />
  <figcaption>A <code>session::fork</code> replay in the console: the forked session copies the parent's transcript up to the chosen entry, then continues independently.</figcaption>
</figure>

## Match the remedy to the cause

To prevent context rot, first identify which of the 4 causes applies to your session, then choose the matching fix.

The table below maps each cause to a remedy, the evidence behind it, and how to apply it in iii.

| Cause found | Remedy | Evidence | In iii |
| --- | --- | --- | --- |
| Old tool results fill most of the context | Trim old results | [Lindenbauer et al. (JetBrains Research)](https://arxiv.org/abs/2508.21433) found that masking old tool outputs roughly halves cost relative to an unmanaged agent, with solve rates matching or occasionally exceeding summarization | `context::prune` replaces eligible outputs with bounded placeholders. Configure `protect_recent_tokens` and `decay_user_turns` to control eligibility |
| Needed fact missing or outdated | Fetch it when needed | [Anthropic's context engineering guidance](https://www.anthropic.com/research/effective-context-engineering-for-ai-agents) recommends just-in-time context: read current state through a function at the turn that needs it | Read current state through a function call rather than relying on the transcript to carry facts forward across many turns |
| The session needs to continue beyond the current token budget | Summarize | [Fan et al.](https://arxiv.org/abs/2609.20804) found that staging rule-based elision before summarization was the most efficient of the tested context-management strategies in coding-agent setups | `/compact keep the ticket ids` runs server-side through the AI Agent Harness, and the compaction marker records where the summary boundary sits |
| A single task fills the context | Split into sub-agents | Anthropic's example architecture uses sub-agents that return condensed summaries of around 1,000 to 2,000 tokens to the orchestrator | The [AI Agent Harness](https://iii.dev/blog/how-to-build-your-own-agent-harness/) spawns sub-agents as isolated child sessions via `harness::spawn`, each with its own [turn loop](https://iii.dev/blog/loops-graphs-and-the-layer-that-matters/) and token budget |
| Context rot was not the cause | Fix the tool, retrieval, or task | Tests 1 through 4 | No context change needed |

One caution on compaction: [Zhang et al. (Agentic Context Engineering)](https://arxiv.org/abs/2510.04618) found that repeated full-context rewriting can collapse accumulated detail. Each compaction round summarizes the older conversation.

After 2 rounds in a session that tracks issue labels, you risk losing the record of which issues were already processed. The compaction guidance string (`/compact keep the ticket ids`) addresses this by telling the summarizer what to preserve, but the safeguard is only as good as the guidance you supply.

## What these tests cannot tell you

**Model differences:** Chroma found that each model family degrades in its own way. None of the 4 conditions showed natural degradation at a 40-issue scale with DeepSeek deepseek-flash and its 1,000,000-token context window. A Claude Sonnet 5.5 pilot on the same task showed the same result.

That is why the run above measures an imposed budget rather than either model's own limits. A result from deepseek-flash does not predict what Claude, GPT-4o, or Gemini will do on the same task. If you switch models, rerun the baseline before drawing conclusions.

**Estimates:** Token counts are estimates until the provider reports actual usage. iii's context-manager estimates tokens for assembly decisions using local counting, and the provider's response supplies exact figures. When the two differ, trust the provider's reported usage for billing and for threshold decisions.

**Single task:** A remedy that worked for issue triage may not transfer to a code migration, a document review, or a multi-step API workflow. The token volumes, tool result sizes, and rule complexity all differ. Run the 4 tests on your specific task before applying a fix.

Across all 4 tests, the approach is the same: find the cause, then make the smallest change that fixes it. Trimming when you should fetch, or compacting when a tool error is driving the failure, adds complexity without addressing the failure.

## What else to know about context rot

### Does a bigger context window fix context rot?

A larger context window delays overflow and gives the model more room before truncation kicks in, but it doesn't guarantee that the model will use long inputs accurately.

[Du et al.](https://arxiv.org/abs/2510.05381) found performance degradation within tested models' advertised limits, meaning the problem can appear well before you hit the ceiling. [Anthropic's context engineering guidance](https://www.anthropic.com/research/effective-context-engineering-for-ai-agents) makes the same point, noting that filling a large window with low-signal content can be worse than a smaller, well-curated context.

### What is an example of context collapse?

Context collapse is what happens when compaction discards detail that the agent still needs. For example, imagine that after 2 compaction rounds in a 40-issue triage session, the summary retains the final label decisions but loses the reasoning about which issues had already received comments.

The agent re-comments on issues it already addressed in turn 15 because that turn's output was absorbed into a summary and then lost in a second compaction. [Zhang et al.](https://arxiv.org/abs/2510.04618) identified this pattern in their analysis of agentic context engineering, noting that repeated full-context rewriting risks erasing accumulated task state.

The fix is to include explicit preservation instructions in the compaction guidance and to verify what the summary retains before continuing the session.

### Does agent memory make context rot worse?

It can, when a memory worker adds outdated facts to every turn. If a memory worker injected a rule or entity state from an earlier session, that content arrives in every assembled context regardless of whether it's still accurate.

In iii, `memory::preview` previews the rules and memories that will be injected before a turn runs. Capture the actual `context::assemble` output for the bad turn to verify what the model received, because the preview and the final assembled input can differ if hooks modify the context after assembly.
