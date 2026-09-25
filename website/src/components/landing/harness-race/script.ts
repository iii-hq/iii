// The race script: what each side types, step by step. Same 8-step story on
// both sides; a step's lines accumulate on top of every earlier step's.

import type { CodeLang } from "@/lib/shiki"

export type LineKind =
  | "prompt"
  | "muted"
  | "ok"
  | "cmd"
  | "warn"
  | "del"
  | "fn"
  | "tool"
  | "read"
  | "ask"
  | "test"
  | "confirm"
  | "choice"
  | "bar"
  | "chips"
  | "worker"

export type Line = {
  k: LineKind
  /** text, or the options of a `choice` / `chips` row */
  t: string | string[]
  /** `fn`: the description under the signature */
  d?: string
  /** worker chip to glow (and latch) once the line has typed */
  hl?: string
  /** badge flashed after a `test` line */
  flash?: string
  /** answer flashed after a `confirm` line */
  ans?: string
  /** code or shell: the text is Shiki-tokenized on the server in this grammar */
  lang?: CodeLang
}

/** A timeline scene. `nat` is the authored length the scene clock is stretched from. */
export type Scene = { name: string; dur: number; step: number; nat?: number }

// Scene list, from the host document's OM_SCENES in the original export.
export const SCENES: Scene[] = [
  { name: "Prompt", dur: 1.3, step: 0, nat: 3 },
  { name: "Find", dur: 2.4, step: 1, nat: 4.5 },
  { name: "Early", dur: 2.8, step: 2 },
  { name: "Iterate", dur: 3.6, step: 3 },
  { name: "Test", dur: 3.2, step: 4 },
  { name: "Worker", dur: 2.4, step: 5 },
  { name: "Ship", dur: 3.2, step: 6 },
  { name: "Result", dur: 4.2, step: 7, nat: 3.5 },
]

/** iii console transcript (right pane). */
export const III: Line[][] = [
  [{ k: "prompt", t: "build a payments ledger service with a durable db" }],
  [
    { k: "muted", t: "Finding workers..." },
    { k: "ok", t: "found existing database worker", hl: "database" },
    { k: "muted", t: "I have all I need, building payments-ledger functions" },
  ],
  [
    { k: "fn", t: "payments::charge::record { amount, customer_id }", d: "Record an authorized charge", lang: "rust" },
    { k: "fn", t: "payments::charge::refund { charge_id }", d: "Issue a full or partial refund", lang: "rust" },
  ],
  [
    { k: "fn", t: "payments::webhook::stripe { event }", d: "Ingest a provider event, idempotent", lang: "rust" },
    { k: "fn", t: "payments::ledger::reconcile { period }", d: "Reconcile the ledger for a period", lang: "rust" },
  ],
  [
    {
      k: "muted",
      t: "Registering functions to http endpoint triggers [/charge/record, /charge/refund, /webhook/stripe, /ledger/reconcile]",
      hl: "http",
    },
    { k: "test", t: "Running tests on dev ... ", flash: "pass" },
    { k: "muted", t: "All tests pass. Building worker" },
  ],
  [{ k: "worker", t: "payments-ledger" }],
  [
    { k: "confirm", t: "Deploy to production? ", ans: "yes" },
    { k: "cmd", t: "compose::add ./payments-ledger --host production", lang: "bash" },
    { k: "ok", t: "joined prod · 25 workers connected" },
  ],
  [],
]

/** Traditional coding agent transcript (left pane). */
export const TRAD: Line[][] = [
  [{ k: "prompt", t: "build a payments ledger service with a durable db" }],
  [
    { k: "muted", t: "I'll research a stack. Searching for options." },
    { k: "tool", t: 'Grep "node orm postgres"', lang: "bash" },
    { k: "tool", t: 'Web "prisma vs drizzle vs typeorm" (12,300 results)', lang: "bash" },
    { k: "muted", t: "Reading blog posts to compare tradeoffs…" },
  ],
  [
    { k: "muted", t: "There are several ORMs. Which do you want?" },
    { k: "choice", t: ["prisma", "drizzle", "typeorm"] },
    { k: "ask", t: "Asking questions…" },
  ],
  [
    { k: "tool", t: 'Bash "npm install prisma @prisma/client"', lang: "bash" },
    { k: "warn", t: "ERESOLVE unable to resolve dependency tree" },
    { k: "muted", t: "Failed to install dependencies for prisma, trying plain postgres" },
    { k: "read", t: "Write src/db/schema.ts  (+190)" },
    { k: "read", t: "Edit src/api-gateway.ts  (+38)  register payments routes" },
  ],
  [],
  [
    { k: "muted", t: "Ready to test locally." },
    { k: "cmd", t: "npm run dev", lang: "bash" },
  ],
  [
    { k: "cmd", t: "git rebase main", lang: "bash" },
    { k: "muted", t: "Rebasing..." },
    { k: "muted", t: "Auto-merging src/api-gateway.ts" },
    { k: "warn", t: "CONFLICT (content): Merge conflict in src/api-gateway.ts" },
    { k: "muted", t: "Automatic rebase failed...resolving merge conflicts" },
  ],
  [],
]

/** Line kinds that type out character by character; the rest fade in. */
export const TYPED: ReadonlySet<LineKind> = new Set([
  "prompt",
  "muted",
  "ok",
  "cmd",
  "warn",
  "del",
  "fn",
  "tool",
  "read",
  "ask",
  "test",
  "confirm",
])

/** Left side's context, in k tokens per step (discrete). */
export const TRAD_TOK = [8, 64, 102, 188, 188, 258, 336, 356]
/** Typing budget per step in seconds. Deliberately not the scene durations: the scene clock is stretched onto these. */
export const DUR = [2.4, 3.6, 2.8, 3.6, 3.2, 2.4, 3.2, 2.8]
/** Staggered think-pauses (fraction of the step); the sides alternate who moves first. */
export const III_DELAY = [0, 0, 0.3, 0, 0.25, 0, 0, 0]
export const TRAD_DELAY = [0, 0.28, 0, 0.25, 0, 0.25, 0.25, 0]

/** Chars per second. iii types a touch quicker than the traditional agent; both brisk, a reader is watching. */
export const TRAD_CPS = 85
export const III_CPS = 115

/**
 * iii runs the same eight steps on a compressed clock: steps 0–6 take 60% of
 * their wall time, so iii is deployed at about 62% of the loop and then holds
 * its final frame while the traditional agent is still working. Both lists
 * sum to the same total, so one playhead drives both windows.
 */
const III_SPEED = 0.6
const TOTAL_DUR = SCENES.reduce((sum, sc) => sum + sc.dur, 0)
export const III_SCENES: Scene[] = (() => {
  const head = SCENES.slice(0, -1).map((sc) => ({ ...sc, dur: Math.round(sc.dur * III_SPEED * 1000) / 1000 }))
  const used = head.reduce((sum, sc) => sum + sc.dur, 0)
  const last = SCENES[SCENES.length - 1]
  return [...head, { ...last, dur: Math.round((TOTAL_DUR - used) * 1000) / 1000 }]
})()
/** Wall-clock second at which iii is deployed. */
export const III_DONE_AT = III_SCENES.slice(0, -1).reduce((sum, sc) => sum + sc.dur, 0)

/** Workers already running in the iii console before the race starts. */
export const BASE_WORKERS = ["http", "state", "database", "queue", "storage"]

/** The status line under each window, per step. The last entry is the verdict. */
export const TRAD_STATUS = [
  "Reading prompt",
  "Researching…",
  "Waiting for an answer",
  "Installing…",
  "Writing files…",
  "Testing locally…",
  "Rebasing…",
  "Still resolving conflicts…",
]
export const III_STATUS = [
  "Reading prompt",
  "Finding workers…",
  "Building functions…",
  "Building functions…",
  "Testing…",
  "Building worker…",
  "Deploying…",
  "Deployed",
]

/** Price per k tokens, for the running spend under each window. */
export const USD_PER_K = 0.011

/** Ceiling of the spend meters, k tokens. Both sides share it, so the bars compare. */
export const MAX_TOK = 360

const clamp01 = (v: number) => Math.max(0, Math.min(1, v))

/**
 * Left side's context at a moment, k tokens: the per-step figures joined into
 * a running count, so the meter climbs rather than jumps.
 */
export function tradTokens(step: number, progress: number) {
  const i = Math.min(step, TRAD_TOK.length - 1)
  const from = i === 0 ? 0 : TRAD_TOK[i - 1]
  return from + (TRAD_TOK[i] - from) * clamp01(progress)
}

/** Right side's context at a moment, k tokens. A small, steady climb that stops once it has deployed. */
export function iiiTokens(step: number, progress: number) {
  const last = SCENES.length - 1
  return 4 + Math.min(step + clamp01(progress), last) * 7
}
