# Narrative framework

A tech spec is organized for completeness. A presentation is organized for
**persuasion**. Do not mirror the spec's table of contents — re-sequence its
material into a story that makes a reader *want* the work to happen. This is
what satisfies requirements 1 (understanding) and 3 (marketing) at once.

## The arc (the default 8–12 slide spine)

1. **Hero — the promise.** Compress the thesis into one line + a three-value
   subhead (simplicity / structure / correctness). Stat strip. Two CTAs. State
   the *win*, never the mechanism. This is the hook. (A1)
2. **Why — the pain.** Name today's failures as concrete, cited cards. Make the
   reader feel the tangle before you offer the fix. (A2)
3. **Show it — the demo.** An interactive proof, early: a CLI playground or a
   sequence. "Run it, step through it" beats "read about it" and converts
   skimmers fast. (A3 / A5)
4. **The map — what exists.** The whole architecture in one navigable diagram.
   Orient the reader before any detail. (A4)
5..N. **The mechanisms — how it works.** One slide per design pillar, each in
   the archetype that fits: sequence for protocols, step-reveal for lifecycles,
   fan-out for reactivity, funnel for "many → one", toggle for policy, decision
   flow for governance. One claim per slide; depth behind a `<SpecSheet>`.
N+1. **The transition — adoption.** Before/after, removed/renamed, a phased
   roadmap. Lower the perceived risk of doing the work. (A11)
Last. **The payoff — why it holds.** A quantified scorecard + a problem→answer
   table. Close the persuasion loop. (A11)

Deep-dive pages (A14) hang off the arc for distinct consumers/scenarios that
deserve a full walkthrough.

## Techniques (apply throughout)

- **Three-value hero framing.** Simplicity + structure + correctness in one
  breath. "one file. one command. zero zombies."
- **Problem → solution pairing.** Never assert a benefit without first naming
  the failure it cures.
- **Before/after wherever available.** Two columns, today vs target. Contrast
  is the most legible form of argument.
- **Ration the accent.** It lands on success / active / CTA only. Overuse kills
  persuasion; scarcity gives each accent moment weight.
- **Progressive disclosure.** Big claim (headline) → supporting cards → deep
  spec (`<SpecSheet>`, closed). One page serves both the exec who skims and the
  engineer who drills.
- **Quantify the win.** Pull the spec's numbers verbatim (files, commands,
  workers, % reductions). If you compute one, say so. Numbers convince.
- **Stay honest.** Ground every claim in the spec. Keep one trade-offs /
  open-questions beat — admitted limits read as credibility, which persuades
  more than a flawless pitch.
- **Let the reader set the tempo.** Scroll-driven reveals, not an autoplay
  carousel. They advance; the deck responds.

## The planning step (Phase 2)

Before writing any code, produce the outline and **show it to the user**:

```
slide  | archetype | the one claim                         | source        | data to pull
-------+-----------+---------------------------------------+---------------+----------------------------
hero   | A1        | one file, one command, zero zombies   | README §1     | thesis, 4 stats, 2 CTAs
why    | A2        | today: 6 ways processes leak          | daemon §2     | 6 failure cards + citations
run-it | A3        | bring the whole stack up in one step  | onboarding §1 | golden-path commands
map    | A4        | four planes, three meeting points     | README §3     | nodes, edges, per-node info
...    | ...       | ...                                   | ...           | ...
payoff | A11       | smaller surface, stronger guarantees  | README §7     | 5 metrics, problem/answer rows
```

This table is where a dry spec becomes a story, and it is far cheaper to revise
here than after scaffolding. Get sign-off, then build.
