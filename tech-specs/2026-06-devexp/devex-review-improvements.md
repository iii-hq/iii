# DX Review — Improvement Plan

**Reviewed:** the 2026-06 devexp spec (README + 6 detail files).
**Method:** design-stage DX audit against the DX first principles. Evidence source is the spec text
(no running product to clock). Scores are projected, not measured.
**Verdict:** strong design, ~7/10 projected DX. A handful of high-leverage gaps separate it from a 9.

---

## The one-line thesis of this review

The spec's *showcase* docs (lifecycle-and-onboarding, worker-compose) promise best-in-class DX. The
*owning* docs (process-daemon, engine-and-gateway, configuration) don't yet specify the messages,
escape hatches, and visibility that make those promises real. **Close the gap between the showcase
and the owners, and this is a 9.**

---

## P1 — fix before implementation locks (high leverage, cheap relative to impact)

### 1. Error DX is showcased but not owned

**Principle:** #5 Fight uncertainty (every error = problem + cause + fix).
`lifecycle-and-onboarding.md` §7 has 9-tier error messages. But the **owning** files have none:
- `process-daemon.md`: no example text for worker-crash, **start-timeout (W161)**, "started but never
  connected", or daemon-death. The data shapes exist (`ProcState`, `last_exit`); the dev-facing
  strings don't.
- `engine-and-gateway.md`: drain "force-halted M" is "surfaced as a `ComposeEvent`" — but *where* the
  developer sees it (in `iii logs`? a separate stream?) is unspecified.

**Fix:** add a dev-facing error-message catalog to `process-daemon.md` and `engine-and-gateway.md`
mirroring the worker-compose §13 catalog (stable code, example string, the fix line). Minimum set:
worker start-timeout, worker-never-connected, repeated-crash-then-failed, drain force-halt summary,
daemon-not-running, daemon-crashed-recovered.

**Lands in:** `process-daemon.md` (new error catalog §), `engine-and-gateway.md` §8 (force-halt
surfacing).

---

### 2. Breaking behavioral changes have no *runtime* communication plan

**Principle:** #2 Credible (upgrades should be boring) · #7 upgrade fear.
Two changes will silently break muscle memory and CI, and the spec only mentions docs:
- **`iii worker add` no longer auto-starts** (`lifecycle-and-onboarding.md` §5.1). Existing scripts
  that `add` then expect a running PID will silently do nothing.
- **`iii worker status` flips from live TUI to one-shot** (`cli-and-functions.md`). Monitors and
  aliases break on upgrade.

**Fix:** specify *runtime* deprecation output, not just doc edits. When a dev runs the old-shaped
command during the coexistence window, print a one-line warning naming the new form
(`note: 'add' no longer auto-starts — run 'iii up' or 'iii worker add --up'`). Commit to ≥1 release
of warnings before the flip in `migration.md`.

**Lands in:** `migration.md` (deprecation-warning policy), `cli-and-functions.md` (status flip phase
gate).

---

### 3. "Which mode am I in?" is invisible during the 6-phase migration

**Principle:** #5 Fight uncertainty · #6 context-switching cost.
For ≥3 releases there are three code paths in the wild (legacy config.yaml, compose, daemon),
gated partly by an **invisible env var** (`III_PROCESS_DAEMON=1`) and partly by filename detection.
A developer with both `config.yaml` and `worker-compose.yml`, or who forgot the env flag, has no way
to answer "what is iii actually doing right now?"

**Fix:** ship `iii doctor` (or fold into `iii info`) that prints the resolved truth: active format
(legacy/compose), daemon mode on/off, bound port, resolved store directory (the compose-relative one,
§13.1), and daemon status. This is the single best uncertainty-killer for the whole migration and is
cheap — it's reading state that already exists.

**Lands in:** `cli-and-functions.md` (new `iii doctor`), referenced from `migration.md`.

---

## P2 — should fix; clear DX wins

### 4. Measure TTHW for real; make the install wait legible

**Principle:** #2 first-five-minutes · #7 speed is a feature.
The 2.1s in the `iii up` mock is orchestration only. The honest first-run TTHW for a new user is
`curl|sh` download + `iii init` + `iii up` *including `npm install` of two workers* — currently
unmeasured. A 30–90s silent `npm install` inside `iii up` reads as "hung" without progress.

**Fix:** (a) commit to a measured TTHW budget in `lifecycle-and-onboarding.md` §1 and verify it once
the path is buildable; (b) show install progress in the `iii up` stream
(`~ math-worker  installing (npm install) …`) so the wait is legible, not a frozen cursor.

---

### 5. Keep configuration to one surface

**Principle:** #1 Usable (intuitive) · Pit of Success.
Per-worker configuration has exactly one surface: `iii worker config set <id> …` → `configuration::*`.
The configuration worker owns each entry end-to-end; the value lives in the store, the worker
registers its own schema + initial value at boot, and there is no compose `config:` field and no
"effective merged view" to confuse it with. Keep it that way — don't reintroduce a second config
command or a compose-side projection that collides on the word "config".

**Fix:** none needed beyond holding the line: `iii worker config` stays the single get/set/edit
surface in `cli-and-functions.md` §10.

---

### 6. Promote the `iii test` recipe to a first-class command

**Principle:** #6 Show code in context · #3 Learn by doing.
Testing is a first-class DX dimension, and the §9.5 recipe is 5 lines of `trap`/`jq`/`--ephemeral`
boilerplate every author will copy-paste and get subtly wrong (forgotten `trap`, leaked store).
The machinery already exists (`--ephemeral`, `--frozen`, `--only`, `trigger --json`).

**Fix:** ship `iii test` that auto-wires `--ephemeral` + `trap down` + `--frozen`. The spec already
flags this as an open question and recommends "document recipe in v1" — disagree: the cost is a thin
clap arm over functions that exist, and it removes a whole class of flaky-teardown bugs.

---

### 7. Make secrets footguns *enforced*, not just documented

**Principle:** #4 escape hatches · Pit of Success (make the wrong thing hard).
`secrets.md` documents the footguns honestly but doesn't prevent them:
- A secret in inline `environment:` is committed to git forever — only *warned* (`W0xx`), not blocked.
- `iii ps` / `worker info` redact store entries but **not** env-borne secrets.

**Fix:** (a) apply the Rule-4 name-heuristic redaction to env values in `ps`/`info`, not just store
entries; (b) keep the inline-secret warning but make it appear at `up` time too, not only `validate`.

---

## P3 — polish; lower leverage but worth a line in the spec

8. **Cognitive-load decision tree.** The resolution rules a dev must hold (later-file-wins env
   precedence, LIVE/RESTART tagging) are individually fine but collectively heavy. One decision-tree
   diagram in `docs/using-iii/worker-compose.mdx` pays for itself. (Principle #1, #7.)

9. **Windows/WSL must fail helpfully.** `process-daemon.md` §12 rules WSL-only for v1. Ensure the
   failure is a clear message ("the iii daemon needs Unix process groups — use WSL"), not a cryptic
   `spawn_owned`/`libkrunfw` error. (Principle #5.)

10. **macOS degraded mode needs a runtime breadcrumb.** The no-subreaper degraded mode (§8.4) is
    honest in the spec but silent at runtime; a re-adopted `owned:false` process after a daemon crash
    has no context. One line in the orphan-sweep output. (Principle #5.)

11. **Ring-buffer log loss isn't surfaced.** After a daemon crash, default `iii logs` returns empty
    with no hint the in-memory ring was wiped. Print "ring buffer reset on daemon restart — use
    `--durable` for history". (Principle #5.)

12. **DX Measurement is a 3/10 — add one feedback seam.** The spec has no instrumentation. Even a
    minimal opt-in TTHW timer on first `iii up` (local-only) would make the headline claim
    verifiable over time. (Principle: chef-for-chefs — you can't improve what you don't measure.)

---

## What's already strong (keep it)

- Golden path collapse (5 cmds/2 terminals → 4 cmds/1 terminal) — champion-tier.
- `depends_on` + L1 readiness gating killing the "Function not found" race — the magical moment.
- Zero-zombies-by-construction — credibility.
- `worker-compose.md` §13 error catalog and §7 lifecycle error showcase — the bar the rest should meet.
- Typed schema with `deny_unknown_fields` + did-you-mean errors — pit of success for the file devs edit.
- `--ephemeral` / `--frozen` / `--only` test primitives — the right building blocks.

---

## Suggested sequencing

1. **P1 (1–3)** before implementation locks — they change contracts (error catalogs, deprecation
   policy, `iii doctor`), so they're cheapest to fix in the spec.
2. **P2 (4–7)** during implementation — measurable wins, mostly additive.
3. **P3 (8–12)** as polish before each phase ships.

Re-run a DX audit against a *running* `iii up` once Phase 3 (the daemon) is buildable — that's when
TTHW, error messages, and the macOS degraded mode become testable instead of inferred.
