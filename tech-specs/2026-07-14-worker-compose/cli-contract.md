# the worker CLI/env contract

A supervisor can only inject what workers agree to read. Today they agree on
nothing:

| Worker | Engine address | Env read |
| --- | --- | --- |
| http / state / storage / database / harness | `--url` flag, default `ws://127.0.0.1:49134` | none |
| llm-router | `--url` flag | `III_WS_URL` |
| shell / codex / devin / email / bridge / lsp / hermes | varies | `III_URL` |
| any SDK (`register_worker`) | explicit code argument | none |

(`workers/http/src/main.rs:28`, `workers/llm-router/src/main.rs:25`,
`sdk/packages/rust/iii/src/lib.rs:96`.)

## The standard

Every worker binary accepts the same three parameters, each with a flag and
an env fallback, flags winning:

| Parameter | Flag | Env |
| --- | --- | --- |
| engine address | `--url` | `III_URL` |
| namespace | `--namespace` | `III_NAMESPACE` |
| configuration | `--config` | `III_CONFIG` (resolved payload or handle — pinned with the config library) |

- `registerWorker()` / `register_worker()` with **no arguments** reads the
  env contract. Explicit arguments still win — but the zero-arg form is the
  documented default, so tutorial code contains no addresses and no
  namespaces, and the same code runs under compose, under `iii worker`, or by
  hand.
- The daemon injects all three into every child (hooks included). Reserved
  keys: a compose file cannot override them.
- Published binary packages get the flags for free once the shared **config
  registration library** lands (one crate/package each SDK wraps — the
  extraction is a listed next step from the 2026-07-13 review, so the fleet
  is not 41 hand-rolled argument parsers).

## Implicit start for packages

With the contract in place, a `package://` binary needs no start script:
compose execs the resolved artifact with the standard flags. That is the
entire start story for the 34/41 fleet workers that today ship no `scripts:`
at all — and why `run` in scripts.md exists only for local workers.

## Migration

Additive: binaries keep their current flags; the standard adds env fallbacks
and normalizes names. The compose daemon only requires the env contract —
old workers run under compose the day they read `III_URL`/`III_NAMESPACE`/
`III_CONFIG`, which the shared library gives them in one dependency bump.
`iii worker` and iii-worker-ops keep working unchanged during coexistence.
