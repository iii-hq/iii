# developer experience overhaul — interactive presentation

An interactive, infographic-style presentation of the
[2026-06 devexp spec](../README.md): the thesis, the four planes + three meeting
points, an interactive CLI **playground**, the process-daemon parent/reap model,
`depends_on` readiness gating, `worker-compose.yml → iii.lock`, the configuration
store, and a full account of **what is being removed / renamed, why, and how it
looks after**.

Built with Vite + React 19 + Tailwind v4, styled with the
[iii Schematic design system](../../../console/DESIGN.md) (reused byte-for-byte).

## Run it

```bash
pnpm install --ignore-workspace   # standalone install (this folder is not a workspace member)
pnpm dev                          # http://localhost:5173
```

> `--ignore-workspace` matters: the folder sits under a pnpm workspace, so a bare
> `pnpm install` would resolve the parent workspace instead of this project.

## Ship it

```bash
pnpm build      # static site in dist/ — host anywhere, no backend
pnpm preview    # sanity-check the production build
```

## Structure

| Route | Content |
|---|---|
| `#/` | the scroll story: hero → why → **playground** → system map → meeting points → daemon → readiness → compose → config → removed/renamed → payoff |
| `#/playground` | the full simulated terminal + the command → function → owner contract + exit codes |
| `#/compose` | the `worker-compose.yml` schema: per-worker field-merge matrix, two-copies example, validation catalog |
| `#/migration` | the six phases, the coexistence bridge, the `config.yaml` → destination mapping, the cloud cutover |

Every diagram is hand-built SVG/markup driven by small data arrays in
`src/content/` — no chart library:

- `src/content/playground.ts` — the golden-path + day-2 command scripts (the playground)
- `src/content/map.ts` — the four-planes nodes/edges + per-node datasheets + the three meeting points
- `src/content/changes.ts` — before/after, removed/renamed, the zombie root-cause table, the readiness contract, the boot sequence, migration phases, the error gallery

Interactive components live in `src/components/diagrams/`: `CliPlayground`,
`SystemMap` (+ datasheet), `MeetingPointsSequence`, `DaemonModel`
(spawn-funnel + supervision tiers), `ReadinessGraph`.

The light/dark toggle persists to `localStorage` (`iii-theme`), matching the
console's convention.

> Source of truth: every fact, command, and identifier is condensed from
> `tech-specs/2026-06-devexp/*.md`.
