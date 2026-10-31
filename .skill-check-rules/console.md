# Console rules

Rules for documenting the iii Console — the web UI that connects to a running engine.

## The Console is ADE

The iii Console is the web UI of ADE, a standalone application published in the registry as `ade`.
A project adds it like any worker (`iii call compose::add worker=ade`), and projects created by the
installer or by `iii project init` include it. The `iii console` command prints a notice that points
to it. Its documentation is in the iii docs (`using-iii/console.mdx`), not Worker Docs.

## Per-page content is console-owned

Each Console UI page renders data from a specific worker, but the *UI documentation* is
console-owned.

Don't push Console UI docs into worker pages — the user is reading "how do I use the Console's
States page" not "how does iii-state work." Conversely, don't push worker mechanics into the Console
doc; link or reference the worker.

## Default ports

- `127.0.0.1:3111` — the `http` worker (`iii-http`): the project's HTTP routes. A doc that says the
  http worker or its container listens on `3111` is correct.
- `127.0.0.1:3112` — the engine's stream WebSocket.
- `127.0.0.1:3113` — ADE: one port that serves the Console UI and proxies the engine WebSocket at
  `/ws`. A doc that says the console serves the UI and proxies the engine WebSocket on `3113` is
  correct.
- `127.0.0.1:49134` — the engine's SDK WebSocket, which workers and ADE connect to.

These ports cross both the iii docs and Worker Docs surfaces; document them on the Console page
(since the Console is the consumer) and let the workers reference back.
