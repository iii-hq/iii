# Lifecycle & Onboarding — the developer journey, day-2 ops, readiness & docs

This is the developer-experience payoff of the whole overhaul: what a developer actually
types and sees, from first install through day-2 operations, error recovery, and CI. It
also pins the two contracts nobody else owns — the **readiness contract** (what "ready"
means for a worker author) and the **testing/CI story** — and closes with the full docs
restructure. The schema lives in [worker-compose.md](worker-compose.md); the PID model in
[process-daemon.md](process-daemon.md); the CLI↔function map in
[cli-and-functions.md](cli-and-functions.md); the config store in
[configuration-and-bootstrap.md](configuration-and-bootstrap.md); phasing in
[migration.md](migration.md).

---

## 1. The thesis: one file, one command, one terminal, zero zombies

Today's hello-world is **install → `iii project init` → `iii` (engine, terminal 1) →
2× `iii worker add` (terminal 2) → `iii trigger`** — roughly five commands across two
terminals, mutating a hand-edited `config.yaml`, with a documented "wait a few seconds or
Function not found" race (`docs/quickstart.mdx:93-96`) and two divergent worker lifecycles
(iii-managed vs run-by-hand).

The target is four commands, one terminal, one declarative file, one supervised process
tree:

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh   # install
iii init                                                       # scaffold worker-compose.yml + sample workers
iii up                                                         # one managed lifecycle: port + all workers
iii trigger math::add_two_numbers a=10 b=20                    # { "c": 30 }
```

- **One declarative file** — `worker-compose.yml` (the only file a human edits; `iii.lock`
  is machine-written). See [worker-compose.md](worker-compose.md).
- **One command** — `iii up` is a top-level alias for `iii worker compose up`; both invoke
  the identical streaming `compose::up` function. The alias costs nothing (it is a clap
  alias) and is the single most important onboarding ergonomic.
- **One terminal** — `iii up` ensures the daemon + engine are running detached, then
  attaches as a log-streaming client. No mandatory second terminal.
- **Zero zombies by construction** — every worker PID is a direct child of the long-lived
  `iii-process-daemon`, `wait()`-reaped on exit. Orphans and zombies are impossible. See
  [process-daemon.md](process-daemon.md).

### Target `iii up` terminal output

```
iii up · worker-compose.yml · port 49134

  resolving workers …
  ✓ math-worker      local   ./workers/math-worker
  ✓ caller-worker    local   ./workers/caller-worker   (depends_on: math-worker)

  starting (topological order) …
  ✓ engine           listening   ws://localhost:49134
  ✓ math-worker      ready       3 functions   pid 48120
  ✓ caller-worker    ready       1 function    pid 48121

  up in 2.1s · 2 workers · ^C to stop · iii logs -f to follow
```

```mermaid
flowchart LR
  A["curl | sh<br/>install"] --> B["iii init<br/>scaffold worker-compose.yml"]
  B --> C["iii up<br/>compose::up (streaming)"]
  C --> D["iii trigger<br/>{ c: 30 }"]
  C -.attach.-> L["iii logs -f"]
```

---

## 2. `iii init` — one scaffolder

Today there are **two** scaffolders with two mental models: `iii project init` (creates
`config.yaml`, `engine/src/cli/project/mod.rs`) and `iii worker init` (creates ONE worker,
explicitly *deletes* `config.yaml`, `crates/iii-worker/src/cli/init.rs` cleanup block
~line 285). They share `scaffolder-core` but pull in opposite directions.

**Decision: collapse to a single `iii init` that is project-first and emits
`worker-compose.yml`, never `config.yaml`.**

```bash
iii init                 # interactive: pick template (bare | quickstart) + name
iii init quickstart      # non-interactive: the two-worker cross-language demo
iii init --bare          # just worker-compose.yml + .gitignore, no sample workers
iii init --single <lang> # one standalone worker (ts|js|py|rust) — the old `iii worker init`
cd quickstart
```

`iii init quickstart` produces:

```
quickstart/
├── worker-compose.yml          # the spine (replaces config.yaml)
├── .gitignore                  # ignores .iii/ and .env*; iii.lock IS committed
├── workers/
│   ├── math-worker/
│   │   ├── iii.worker.yaml      # per-worker manifest (ships with the worker)
│   │   └── math_worker.py
│   └── caller-worker/
│       ├── iii.worker.yaml
│       └── src/worker.ts
└── iii.lock                    # written on first `iii up` (resolved versions + hashes)
```

The scaffolded `worker-compose.yml` is deliberately minimal:

```yaml
version: "1"
port: 49134

workers:
  math-worker:
    runtime: { workspace: ./workers/math-worker }
  caller-worker:
    runtime: { workspace: ./workers/caller-worker }
    depends_on: [math-worker]
```

What is NOT here vs today's quickstart `config.yaml`: no `iii-worker-manager` entry (the
port is the marquee top-level scalar — see [engine-and-gateway.md](engine-and-gateway.md)),
no `iii-exec` block, no per-worker `config:` nesting. Capability workers (`iii-state`,
`iii-http`) are added later (§5), not front-loaded.

**Scaffolder mechanics.** Reuse `scaffolder-core::TemplateFetcher`
(`crates/scaffolder-core/src/templates/fetcher.rs`); the templates repo `iii-hq/templates`
swaps its shared `config.yaml` file for `worker-compose.yml`. `iii init --single`
replaces the old `iii worker init`; because there is no `config.yaml` to delete, the
config.yaml-deletion hack (`crates/iii-worker/src/cli/init.rs` ~line 285) becomes moot.

**`.gitignore` and secrets.** `iii init` scaffolds a `.gitignore` that ignores `.iii/`
and `.env*` and **commits** `iii.lock`. Secrets never go in `worker-compose.yml`; they
flow through `env_file` (gitignored) or `${VAR}` expansion. See [secrets.md](secrets.md).

---

## 3. `iii up` semantics (reconciled with canon)

`iii up` runs in the **foreground by default**, but it is **not** the process parent. This
is the precise layering (resolves the F-vs-C tension flagged in critique A7):

| Step | Who | What |
|---|---|---|
| 1 | CLI | Ensure `iii-process-daemon` + engine are running, **detached and long-lived**. Spawn them if absent. |
| 2 | CLI → worker-ops | Invoke streaming `compose::up`; render `ComposeEvent` progress. |
| 3 | worker-ops | Topo-sort the graph, resolve, call `process::start` per node, gate each on `status::watch_until_ready` (`crates/iii-worker/src/cli/local_worker.rs:588`). |
| 4 | CLI | **Attach as a log-streaming client** and install a **SIGINT handler that invokes `compose::down`**. |

The behavioral contract that follows:

- **Ctrl-C (SIGINT)** = clean teardown. The CLI *actively calls* `compose::down`; teardown
  is an explicit action, not a side effect of the CLI exiting. This preserves
  docker-compose muscle memory.
- **`kill -9` of the `iii up` CLI** = workers **keep running**. The daemon owns them; the
  CLI was only a viewer. `iii down` (or a later `iii up`) reconciles.
- **`iii up -d` / `--detach`** = same as foreground minus the attach and minus the
  SIGINT→down handler. Frees the terminal; `iii down` stops the stack.

Because the daemon is always detached and survives engine hot-reload (it is a separate
long-lived process — see [process-daemon.md](process-daemon.md)), `iii up` never becomes
the supervisor. The drain/restart protocol it relies on lives in
[engine-and-gateway.md](engine-and-gateway.md).

**`depends_on` kills the "Function not found" race.** `compose::up` does not start
`caller-worker` until `math-worker` reports **ready** (process up + WS-connected +
functions registered — see the readiness contract in §8). The default `depends_on`
condition is `ready`, strictly stronger than Docker's `started`. Validation (missing refs,
cycles) happens up-front during graph build; nothing starts if the graph is invalid.

---

## 4. Hot-reload journey

```bash
# edit workers/math-worker/math_worker.py, save
```

What the dev sees in the `iii up` / `iii logs -f` stream:

```
  ~ math-worker      source changed   restarting …
  ✓ math-worker      ready            3 functions   pid 48140
  ~ caller-worker    dependency restarted   (no restart needed)
```

Semantics:

- Local `workspace:` workers are watched (the `__watch-source` sidecar,
  `crates/iii-worker/src/cli/local_worker.rs:1137` — today detached, now a daemon-owned
  child). On change, setup/install are skipped if the `.iii-prepared` marker is unchanged
  (`local_worker.rs:182-280`); only `start` is re-exec'd.
- **The daemon owns the restart**, so the old PID is `wait()`-reaped *before* the new one
  starts — no leak, no zombie. The drain protocol (let in-flight invocations finish before
  swapping the connection) is specified in [engine-and-gateway.md](engine-and-gateway.md).
- **Dependents are NOT auto-restarted** unless the worker's *interface* changed (the set of
  registered functions differs). Keep it cheap: a behavioral edit to `math-worker` that
  keeps the same functions does not bounce `caller-worker`.

---

## 5. The real-app journey: remote packages, two copies, wiring

Everything past hello-world is one edit to `worker-compose.yml` + `iii up`, or the
imperative equivalent.

### 5.1 Add a capability worker — declarative by default

```bash
iii worker add iii-state         # appends to worker-compose.yml + iii.lock
iii worker add iii-http
iii worker add observability     # remote: workers.iii.dev/observability:latest
```

`iii worker add <name>` resolves `<name>` to `package: workers.iii.dev/<name>:latest`,
writes the resolved concrete version + sha256 into `iii.lock` (keyed by
`(package, version)` so two divergent copies are representable), and adds a minimal entry
to `worker-compose.yml`:

```yaml
version: "1"
port: 49134
workers:
  math-worker:    { runtime: { workspace: ./workers/math-worker } }
  caller-worker:  { runtime: { workspace: ./workers/caller-worker }, depends_on: [math-worker] }
  state:          { runtime: { package: workers.iii.dev/iii-state:latest } }
  http:           { runtime: { package: workers.iii.dev/iii-http:latest } }
```

**Decision (BREAKING vs today): `add` is declarative-only and does NOT auto-start.** Today
`add` mutates `config.yaml` AND auto-starts a PID (`docs/quickstart.mdx:36`). In the new
model `add` only edits the file + lock; `iii up` (or `iii worker start <id>`) reconciles
running state. There is exactly ONE source of truth (worker-compose.yml + iii.lock) and
ONE way to make reality match it. For convenience, `iii worker add --up` reconciles
immediately. This is gated behind compose-mode during migration (see
[migration.md](migration.md)); the `AddOptions.source` → `sources: Vec` change is a
net-new breaking function-schema change, versioned.

### 5.2 Two copies of the same package (distinct ids)

```yaml
workers:
  http-public:
    runtime: { package: workers.iii.dev/iii-http:latest }
    environment: { LISTEN_PORT: "3111" }
  http-internal:
    runtime: { package: workers.iii.dev/iii-http:latest }
    environment: { LISTEN_PORT: "3211" }
```

The id (`http-public`) is the stable name everywhere — logs, `ps`, `exec`, and the
configuration entry it registers at boot. The package is identical; the daemon supervises
two children, distinguished by id (not by config files). Each registers its own config
under its own id via `configuration::register`. Duplicate semantics are owned by
[worker-compose.md](worker-compose.md).

### 5.3 Wiring depends_on + env — the two resolution rules

```yaml
workers:
  caller-worker:
    runtime: { workspace: ./workers/caller-worker }
    depends_on: [math-worker, state]      # waits for BOTH to be ready
    environment:
      LOG_LEVEL: debug                     # overrides iii.worker.yaml env
    env_file:
      - .env                               # lower precedence
      - .env.local                         # LATER file wins
```

A developer must internalize exactly two rules (kept deliberately small):

1. **Override:** any `worker-compose.yml` field overrides the worker's shipped
   `iii.worker.yaml` (deep-merge maps like `environment`/`scripts` by key;
   replace scalars and lists like `depends_on`/`env_file`). Configuration is *not* a merged
   field — it lives in the `configuration` worker, which each worker registers with at boot.
2. **Env precedence (later-wins):** the full ladder, highest → lowest, is
   **host process env > inline `environment:` > `env_file[n]` > … > `env_file[0]`**. Among
   files, the **last-listed file wins**. (This supersedes the mission's "lowest in list
   wins" phrasing; see [worker-compose.md](worker-compose.md).)

### 5.4 Bring it up

`iii up` is the same one command. Output now shows the dependency-ordered bring-up of
`state` → `http` → `math-worker` → `caller-worker`, each gated on readiness. The developer
never thinks about the WS port, pidfiles, or install timing.

---

## 6. Day-2 operations

Every command is a thin wrapper over a `worker::*` / `process::*` / `compose::*` /
`configuration::*` function (see [cli-and-functions.md](cli-and-functions.md)) over the
same WS transport `iii trigger` uses.

```bash
# Status — scriptable data dumps (NEW info + ps, not the live TUI)
iii ps                                   # process table: id, source, state, pid, uptime, restarts, fns
iii worker info caller-worker            # static + resolved: source, version, deps, env, configuration entry id
iii worker status                        # compose-wide health rollup (one-shot)
iii worker status caller-worker --watch  # opt-in live TUI

# Logs
iii logs                                 # all workers, interleaved, color-keyed by id
iii logs -f                              # follow
iii logs caller-worker --since 5m        # one worker, time-bounded

# Lifecycle (single worker, no full down/up)
iii worker restart caller-worker
iii worker stop  state
iii worker start state

# Exec into a worker (host process or its sandbox/VM)
iii worker exec caller-worker -- sh
iii worker exec caller-worker -- npm run migrate

# Config (the configuration worker, not a file)
iii worker config caller-worker                       # show resolved config blob (secrets redacted)
iii worker config caller-worker --set LOG_LEVEL=trace # live set via configuration::set
iii worker config caller-worker --edit                # open $EDITOR, write back
```

`iii ps` target output:

```
ID              SOURCE   STATE    PID     UPTIME   RESTARTS   FNS
math-worker     local    ready    48120   4m12s    0          3
caller-worker   local    ready    48121   4m10s    0          1
state           remote   ready    48118   4m20s    0          5
http-public     remote   ready    48130   4m20s    1          2
```

Day-2 design decisions:

- **`info` and `ps` are data dumps** — non-interactive, scriptable, `--json`-able. `status`
  keeps the live TUI but behind `--watch`. This fixes the "status is a TUI not a data dump"
  gap.
- **One-shot vs live:** `ps`/`info` are point-in-time; `status --watch` is the streaming
  surface the [TUI](migration.md) (`tuiii`) consumes.
- **`iii worker config`** is per-worker get/set/edit backed by
  `configuration::{get,set,list,schema}`. Each worker owns its config end-to-end: it
  registers its own schema + initial value at boot via `configuration::register`, and the
  configuration worker persists the value. Live `--set` validates against the JSON schema;
  restart-tier fields warn "requires restart". Secrets are redacted as `***` unless
  `--reveal` is passed (see [secrets.md](secrets.md)).
- **`iii sandbox …`** stays as-is for explicit sandbox/microVM ops, backed by the unchanged
  16 `sandbox::*` functions.

---

## 7. Error & recovery DX

Daemon ownership exists to make failure *legible*. Concrete messages:

### 7.1 Crashed worker (auto-restart with backoff)

```
  ✗ caller-worker    exited (code 1)   restarting (1/5) …
  ✓ caller-worker    ready             1 function   pid 48155

  caller-worker crashed once. Last 10 log lines:
    TypeError: cannot read property 'add' of undefined
    …
  → iii logs caller-worker --since 1m   for the full trace
```

The daemon is the direct parent, so it observes the exit immediately (no `kill(pid,0)`
polling), records it in the authoritative process table (the `RESTARTS` column), and
applies bounded exponential backoff. After N failures it stops and marks the worker
`failed` rather than thrashing. See [process-daemon.md](process-daemon.md).

### 7.2 Zombies — impossible by construction (no recovery UX)

There is no "zombie recovery" UX because there are no zombies. The structural root cause
(detach with `setsid()` + return immediately + pidfile handoff) is removed: every worker
is a tracked child of the long-lived daemon, `wait()`-reaped on exit. The only related UX
is the **startup orphan sweep**: if the daemon itself was `kill -9`'d, the next `iii up`
re-adopts surviving children by `instance_token` (or kills them) and prints:

```
  ! found 2 orphaned worker processes from a previous run — reclaimed
  ✓ math-worker      ready   (reattached pid 48120)
  ✓ caller-worker    ready   (reattached pid 48121)
```

### 7.3 Missing dependency (did-you-mean)

```
  ✗ compose error: worker "caller-worker" depends_on "mathh-worker" which is not declared

    declared workers: math-worker, caller-worker, state
    did you mean "math-worker"?

  → fix depends_on in worker-compose.yml
```

Validated up-front during graph build. Nothing starts on an invalid graph — fail fast, no
half-up state.

### 7.4 depends_on cycle

```
  ✗ compose error: dependency cycle detected
    caller-worker → state → caller-worker
  → break the cycle in worker-compose.yml
```

### 7.5 Port conflict

```
  ✗ cannot bind ws://localhost:49134 — address already in use

    another iii is likely running here. Options:
      iii down                 stop the existing stack
      iii ps                   see what's running
      edit worker-compose.yml  change top-level `port:`
```

The port is one top-level field, so the error points at exactly one place. A *worker's*
listener port conflict (e.g. http `3111`) names the worker id and its `environment` source
(or the configuration worker).

### 7.6 Install / version failure (lock fallback)

```
  ✗ state   failed to resolve workers.iii.dev/iii-state:latest (network)
            using last-locked version from iii.lock: iii-state@1.4.2
  ✓ state   ready (offline, locked)
```

The committed `iii.lock` lets `iii up` proceed offline with the last resolved version.
`iii up --frozen` (CI) refuses any drift from the lock (§9).

---

## 8. The readiness contract (define it — nobody else did)

`depends_on` is sold as the fix for the "Function not found" race, so "ready" must be
**defined for a worker author**, not just asserted. Readiness is a three-level contract;
the author opts up.

| Level | Name | Met when | Detected by |
|---|---|---|---|
| **L0** | spawned | process has a PID | daemon process table |
| **L1** | connected | WS-connected **and** functions registered | engine registry + `status::watch_until_ready` (`local_worker.rs:588`) |
| **L2** | healthy | a worker-declared `healthcheck:` passes | daemon poll of the declared probe |

**v1 default = L1.** `depends_on: [x]` waits until `x` reaches L1. An opt-in
`condition: started` (L0) and a `healthcheck:` block (L2) are the escape hatches.

```yaml
workers:
  model-server:
    runtime: { package: workers.iii.dev/model-server:latest }
    healthcheck:
      function: health::check      # a function the worker exposes; returns ok/not-ok
      interval: 5s
      timeout: 2s
      retries: 10
      start_period: 30s            # grace before failures count (slow warm-up)
  api:
    depends_on:
      model-server: { condition: healthy }   # wait for L2, not just L1
```

**How an author declares readiness.** Two supported paths:

1. **Stay not-ready (recommended for slow warm-up).** A worker that needs to load a model
   or open a pool simply does not register its functions until warm. L1 then naturally
   reflects functional readiness — no extra surface.
2. **Declare an L2 `healthcheck:`** that calls a worker-exposed function (e.g.
   `health::check`) or runs a command. Use this when the worker must register functions
   early (to be discoverable) but is not yet serving traffic.

The `healthcheck:` block schema is owned by [worker-compose.md](worker-compose.md); the
daemon poll mechanics by [process-daemon.md](process-daemon.md).

**Healthcheck FAIL of an already-running worker.** Decision: a worker that was healthy and
then fails its healthcheck `retries` times in a row is **marked `unhealthy` but left
running** (state shows `unhealthy` in `ps`/`status`; it is NOT restarted automatically).
Rationale: an unhealthy-but-alive worker may be mid-recovery (reconnecting to a DB), and a
restart loop is more disruptive than a visible `unhealthy` flag. An explicit
`healthcheck.on_fail: restart` opts into restart-on-unhealthy for workers that prefer it.
Process *exit* (a real crash) is always handled by the backoff/restart policy in §7.1 —
this rule is only about alive-but-unhealthy.

### Open questions

- **L2 probe transport.** Two options: (a) a worker-exposed `health::check` function the
  daemon calls over WS, or (b) an in-VM/host command. Recommended default: **(a) for
  connected workers** (uniform with the function bus, no shell needed), **(b) only inside
  sandboxes**. Lead author to confirm whether `health::check` becomes a reserved function
  name.
- **`unhealthy` and `depends_on`.** If `api` depends on `model-server: { condition: healthy }`
  and `model-server` later goes `unhealthy`, do we stop routing to it? Recommended: surface
  it, do not auto-stop dependents (matches the "mark, don't cascade" stance above).

---

## 9. Testing / CI for worker authors

DX includes the test loop, which no prior design covered. The primitives already exist;
this assembles them into a recipe.

### 9.1 Partial bring-up

```bash
iii up --only caller-worker        # bring up caller-worker + its depends_on closure
iii up math-worker state           # explicit subset (compose::up [W…])
```

`--only <id>` resolves the `depends_on` closure so the worker under test gets its real
dependencies and nothing else.

### 9.2 Ephemeral mode (isolation from a running dev stack)

```bash
iii up --ephemeral --only caller-worker
```

`--ephemeral` uses a **temp `configuration` store directory + a random free `port`** so a
test run never collides with a developer's live `iii up` on `49134`. On teardown the temp
store is discarded. This is the safe default for CI and for `git`-hook test runs.

### 9.3 Assertions

`iii trigger fn k=v --json` is the assertion primitive — it prints the raw function result
as JSON to stdout, so a test asserts on it directly:

```bash
result=$(iii trigger math::add_two_numbers a=10 b=20 --json)
echo "$result" | jq -e '.c == 30'
```

### 9.4 Lock-drift gate

```bash
iii up --frozen     # CI: refuse to run if resolution would differ from iii.lock
```

`--frozen` fails the build if `worker-compose.yml` would resolve to anything not already
pinned in the committed `iii.lock` — the reproducible-build gate.

### 9.5 A thin `iii test` recipe

A minimal `package.json` / `Makefile` target, not new machinery:

```bash
# scripts/test.sh
set -euo pipefail
iii up --ephemeral --frozen --only caller-worker
trap 'iii down --ephemeral' EXIT
iii trigger math::add_two_numbers a=10 b=20 --json | jq -e '.c == 30'
```

**Open question.** Whether to ship this as a first-class `iii test` subcommand (auto-wires
`--ephemeral` + `trap down`) or leave it as a documented recipe. Recommended: document the
recipe in v1; promote to `iii test` only if authors ask.

---

## 10. Before / after

| Dimension | Today (friction) | Target |
|---|---|---|
| **Files to understand** | `config.yaml` (hand-edited + command-mutated) + `iii.lock` + per-worker `iii.worker.yaml`; SDK example configs 80–126 lines | One `worker-compose.yml` (port + workers map) + auto-managed `iii.lock`; per-worker config in the `configuration` worker |
| **Commands to first call** | install → `project init` → `iii` (T1) → 2× `worker add` (T2) → `trigger` ≈ 5 cmds / 2 terminals | install → `iii init` → `iii up` → `iii trigger` = 4 cmds / 1 terminal |
| **Terminals** | ≥2 (foreground engine + ops tab) | 1 (`iii up` foreground, or `-d`) |
| **Lifecycle** | foreground engine; no up/down; `add` auto-starts PIDs | `iii up`/`iii down`; declarative reconcile; `add` edits file only |
| **Startup race** | "wait a few seconds or Function not found" | `depends_on` + L1 readiness gating; no race |
| **Process safety** | zombies/orphans; pidfile + `ps`-scan; two divergent lifecycles | daemon is direct parent of all; reaped on exit; one lifecycle; zombies impossible |
| **Ports** | WS port hidden in `iii-worker-manager`; data ports buried in per-worker config | top-level `port:`; worker ports in that worker's `environment` (or the configuration worker), surfaced by `iii ps`/`info` |
| **CLI verb surface** | 30+ leaf commands | consolidated `add/update/remove/clear/list/info/start/stop/restart/logs/status/ps/exec/config` + `compose {up,down,restart,status,validate}` |
| **Scaffolding** | `project init` (makes config.yaml) vs `worker init` (deletes config.yaml) | one `iii init` (emits worker-compose.yml); `--single` for one worker |
| **Lockfile on day 1** | sync/verify/drift in the quickstart | `iii.lock` is invisible plumbing; surfaces only in CI (`--frozen`) and `info` |
| **Arbitrary processes** | `iii-exec` buried in config.yaml | `process::start{spec, watch}` on the daemon; declared in compose |
| **Config is…** | hand-edited YAML blocks per worker | `iii worker config <id>` over the `configuration` worker (get/set/schema-validated) |
| **Readiness** | undefined; "wait a few seconds" | L0/L1/L2 contract; default L1; `healthcheck:` escape hatch |
| **Testing/CI** | undocumented | `up --only` + `--ephemeral` + `trigger --json` + `--frozen` |
| **CLI ↔ function parity** | partial: 7 verbs function-backed | every verb is a `worker::*`/`process::*`/`compose::*` function; CLI is a thin wrapper |

---

## 11. Docs restructure

Grouped by action. Paths are repo-relative under `/Users/sergio/Documents/workspaces/iii/iii/`.

### Rewrite (config.yaml → worker-compose.yml + compose lifecycle)

- `docs/quickstart.mdx` — replace steps 2–7 (`iii --config config.yaml`, every
  `iii worker add`, the 2-terminal "keep this open") with `iii init` → `iii up` →
  `iii trigger`. (Lines 52–236.)
- `docs/using-iii/engine.mdx` — gut the config.yaml sections ("Engine configuration",
  "Configuration file structure", "Env var expansion", "Default configuration",
  lines 15–83); replace with worker-compose.yml + the top-level `port`.
- `docs/using-iii/workers.mdx` — rewrite "Managing workers" / "Adding a worker"
  (config.yaml write at line 62), demote the lockfile section (153–186) to "Reproducible
  installs / CI", and retarget start/stop/status/logs/exec verbs (80–106) to the
  consolidated CLI.
- `docs/using-iii/cli.mdx` — replace the subcommand table (36–46) with the consolidated
  verb set + `iii up`/`iii down`/`iii worker compose …`; document CLI↔function parity.
- `docs/creating-workers/workers.mdx` (433–461) & `docs/creating-workers/worker-manifest.mdx`
  (8–12) — switch the "engine reads manifest from config.yaml" narrative to the
  worker-compose override-precedence model.
- `docs/understanding-iii/engine.mdx`, `docs/understanding-iii/index.mdx` — adjust the
  worked example to the compose model.
- `docs/using-iii/deployment.mdx` — the generate-Docker-assets flow assumes config.yaml;
  retarget to worker-compose.yml + `iii.lock`.
- `docs/install.mdx` — resolve the binary-download TODO (line 8); drop the
  "auto-downloads iii-worker" wrinkle once single-binary lands.

### Add (new pages)

- `docs/using-iii/worker-compose.mdx` — the new spine page: file schema (version, port,
  gateway, configuration, defaults, workers; runtime workspace/package, scripts,
  depends_on, environment, env_file, healthcheck), override semantics, env
  precedence, duplicate-id. Mirrors
  [worker-compose.md](worker-compose.md).
- `docs/using-iii/lifecycle-and-processes.mdx` — up/down, foreground vs `-d`, hot-reload,
  the daemon ownership model, crash/backoff, orphan sweep, the readiness contract, and
  "why no zombies". Mirrors this doc + [process-daemon.md](process-daemon.md).
- `docs/using-iii/configuration.mdx` — the `configuration` worker as the config store;
  `iii worker config get/set/edit`; LIVE vs restart-tier fields; secret redaction. Mirrors
  [configuration-and-bootstrap.md](configuration-and-bootstrap.md) + [secrets.md](secrets.md).
- `docs/using-iii/testing.mdx` — the §9 recipe: `up --only`, `--ephemeral`,
  `trigger --json`, `--frozen`.
- `docs/reference/cli.mdx` and `docs/reference/functions.mdx` — generated parity tables:
  each CLI verb and its backing `worker::*`/`process::*`/`compose::*` function.

### Archive (remove / supersede)

- `docs/0-11-0/workers/iii-exec.mdx` — folded into the process-daemon page.
- `docs/0-11-0/workers/iii-worker-manager.mdx` — folded into compose `port` + the engine
  gateway.
- `docs/0-11-0/workers/worker-management-triggers.mdx`,
  `docs/0-11-0/how-to/create-ephemeral-worker.mdx`,
  `docs/0-10-0/how-to/create-ephemeral-worker.mdx`.
- `docs/tutorials/linkly/frontend.mdx` (+ `next/`, `0-18-0/` copies) — port off `iii-exec`.

### Port (examples & templates)

- `sdk/packages/node/iii-example/config.yaml`,
  `sdk/packages/python/iii-example/config.yaml` (incl. their `iii-exec` blocks) →
  `worker-compose.yml`.
- Templates repo `iii-hq/templates`: swap the shared `config.yaml` for `worker-compose.yml`
  in both `bare` and `quickstart`; delete the `iii worker init` config.yaml-cleanup hack
  (`crates/iii-worker/src/cli/init.rs` ~line 285) — moot.
