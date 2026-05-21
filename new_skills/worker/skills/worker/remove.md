---
type: how-to
function_id: worker::remove
title: Uninstall workers from iii.config.yaml
---

# When to use

Call `worker::remove` to drop a worker's entry from `iii.config.yaml`.
The engine's file watcher picks up the change and tears down any running
sandbox for that worker. Cached artifacts under
`~/.iii/managed/{name}/` are **left on disk** — call
[`worker::clear`](iii://worker/clear) afterwards to reclaim that space.
The call is idempotent: a second remove with the same input returns an
empty `removed` list.

Reach for it when:

- You're decommissioning a worker and want it gone from the engine's
  surface (but might re-add later, so the cache is still useful).
- A config file is drifting and you want to rebuild from a known
  `worker::add` baseline.
- You're scripting cleanup and need a destructive op that explicit
  consent (`yes: true`) gates.

Use [`worker::clear`](iii://worker/clear) instead when you want to free
disk but keep the config entry (e.g. forcing a fresh download on the
next `worker::start`). Use [`worker::stop`](iii://worker/stop) instead
when you only want to halt the process without touching its config.

# Inputs

```json
{
  "names":  ["image-resize"],  // explicit list of worker names to remove; mutually exclusive with `all`
  "all":    false,             // optional; true removes every installed worker (mutually exclusive with `names`)
  "yes":    true               // required consent flag; must be exactly true
}
```

Pass either `names` (a non-empty array) **or** `all: true` — never both,
never neither (**W103**). `yes` must be exactly `true` (not the string
`"true"`, not the number `1`, not omitted), or the call returns **W104**.
Names that don't match `[a-z0-9_-]{1,64}` return **W100**; names not
present in `iii.config.yaml` are silently skipped (they show up as
absent from the `removed` list).

# Outputs

```json
{
  "removed": ["image-resize"]  // worker names that were actually removed; empty when nothing changed
}
```

- `removed` lists the names that were actually purged from
  `iii.config.yaml`. Names that were already absent are silently dropped
  from the list, so a second remove of the same names returns `[]`.
- The list is sorted in the input order for `names` requests, and
  lexicographically for `all: true` requests.

# Side effects

- Mutates `iii.config.yaml` in place (removes the named entries).
- The engine's file watcher observes the change and tears down any
  running sandbox tied to the removed worker.
- Does **not** touch `~/.iii/managed/{name}/` — disk usage is unchanged
  until [`worker::clear`](iii://worker/clear) is called.
- Does **not** rewrite `iii.lock`; the pinned versions linger until a
  subsequent `worker::add` rewrites the lock.

# Worked example

Remove a single worker with explicit consent:

```json
{ "names": ["image-resize"], "yes": true }
```

Remove several workers at once:

```json
{ "names": ["image-resize", "skills"], "yes": true }
```

Remove every installed worker (a full reset):

```json
{ "all": true, "yes": true }
```

# Related

- `worker::clear` — free the cached artifacts left behind on disk.
- `worker::list` — verify the entries are gone after the call.
- `worker::add` — re-install a worker that was just removed.

## Errors

- **W103** `names` empty and `all` unset, or both set.
- **W104** `yes` is not `true`.
- **W900** filesystem failure writing `iii.config.yaml`.
