# lifecycle — up, readiness, cascade, teardown

The daemon owns its children; the engine is the source of truth for
readiness. Every lifecycle answer follows from those two facts.

## up

1. Validate file + dependency DAG (offline ruleset — cycles print their full
   path).
2. Resolve every container's config (fetch base + merge overrides). Any fetch
   failure fails `up` before a single process exists.
3. Walk the DAG in topological order. Per container:
   `pre_start` (blocking, timed) → spawn `run` in its own process group /
   Job Object → wait for **readiness**.
4. On a startup failure: capture the failing worker's logs, roll back the
   containers **this operation** started (reverse order), leave everything
   that was already running untouched.
5. `up` against an already-running file is a no-op success (`changed: false`
   per container).

**Readiness = the worker's registration is visible on the engine** — not a
live PID. A process that boots but never registers is a startup failure at
its `startup_timeout`, and the client timeout of the invoking `iii trigger`
must sit above the graph's budget (a 30s client default under a 60-90s
startup budget loses the result mid-operation).

## Optimistic trigger buffer

Startup order breaks registrations today: a worker that registers a route
before its trigger-type worker (e.g. the http router) is up gets a warning
and a dropped route. The engine gains a **buffer**: trigger registrations
that reference an absent worker wait in memory and flush when that worker
arrives (or re-arrives after a restart). Compose exposes the knobs — how long
a registration may wait, how many retries — so the buffer cannot silently
hold a route forever. Ordering becomes a performance concern, not a
correctness one; `depends_on` stays about *data* dependencies, not
registration order.

## Cascading failure

If a worker crashes post-ready, its local transitive dependents stop, in
reverse dependency order, states set to `failed`, cause in the logs. The
alternative — a queue worker happily pushing into a dead database — corrupts
work invisibly. Softer edges (systemd-style `wants` vs `needs`) can come
later for optional side-processes; v1 has one edge type and it is strict.

## down

- `down` (bare): stops everything this daemon owns, dependents before
  dependencies. Never anything owned by another daemon.
- `down <id>`: the target plus its local transitive dependents.
- Termination per child: SIGTERM to the process group → bounded grace →
  SIGKILL (Job Object termination on windows). Claims/registrations release
  only after the process is confirmed dead. Once an exit is confirmed the
  child's `post_run` fires (not awaited — teardown proceeds).
- Intentional daemon shutdown (SIGINT/SIGTERM) performs a local `down`.
  Engine disconnect does **not** stop children: they keep serving and the
  daemon reconciles when the engine returns.

## Crash and restart

State on disk per child (pid + process birth identity + group id + log
paths). On daemon restart: verify each recorded process against its birth
identity — reconcile the live ones, clean the dead ones, never signal a pid
it cannot verify (pid recycling is real). A worker that dies while the daemon
is down is detected here and cascades then.

## Remote surface

`compose::up / down / list / status / logs / validate` — one function family,
registered by the daemon like any worker's; the target daemon travels as an
argument:

```bash
iii trigger compose::up id=host-a
iii trigger compose::down id=host-a
iii trigger compose::status id=host-a
```

`list`/`status` answer for **the addressed daemon's compose project only**;
the engine-wide view stays on `worker::list` (which is also being unified:
every connected entity reports as a worker with full metadata).
